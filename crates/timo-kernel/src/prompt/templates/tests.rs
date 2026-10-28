//! prompt 模板测试（设计 phase-3.5/09 §6 的 T1~T4）。

use std::path::{Path, PathBuf};

use super::*;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn substitute(content: &str, values: &[&str]) -> String {
    super::substitute(content, &args(values))
}

#[test]
fn positional_all_and_default_placeholders_substitute() {
    let given = ["one", "", "three"];
    assert_eq!(substitute("$1/$2/$3", &given), "one//three");
    assert_eq!(substitute("$0 $4", &given), " ");
    assert_eq!(
        substitute("$@ | $ARGUMENTS", &given),
        "one  three | one  three"
    );
    assert_eq!(substitute("${2:-fallback}", &given), "fallback");
    assert_eq!(substitute("${9:-missing}", &given), "missing");
    assert_eq!(substitute("${@:-nothing}", &[]), "nothing");
    assert_eq!(substitute("${@:-nothing}", &["a"]), "a");
    // 目标名只认 @ / ARGUMENTS / 数字；其它写法整段原样（蓝本正则不匹配）。
    assert_eq!(substitute("${FOO:-x} ${:-y}", &given), "${FOO:-x} ${:-y}");
}

#[test]
fn slices_are_one_indexed_with_zero_treated_as_one() {
    let given = ["a", "b", "c", "d"];
    assert_eq!(substitute("${@:3}", &given), "c d");
    assert_eq!(substitute("${@:2:2}", &given), "b c");
    assert_eq!(substitute("${@:0:2}", &given), "a b");
    assert_eq!(substitute("${@:9}", &given), "");
    assert_eq!(substitute("${@:2:99}", &given), "b c d");
}

#[test]
fn arguments_split_on_whitespace_but_not_inside_quotes() {
    assert_eq!(
        parse_args(r#"one "two three" '' four"#),
        args(&["one", "two three", "", "four"])
    );
    assert_eq!(parse_args("'single words'"), args(&["single words"]));
    assert_eq!(
        parse_args("  leading   and  trailing  "),
        args(&["leading", "and", "trailing"])
    );
    // 未闭合引号走到文末（蓝本不报错）。
    assert_eq!(parse_args("\"unclosed tail"), args(&["unclosed tail"]));
    assert_eq!(parse_args(""), Vec::<String>::new());
}

#[test]
fn argument_values_are_not_expanded_again() {
    assert_eq!(substitute("$1", &["$2 $@"]), "$2 $@");
    assert_eq!(substitute("${1:-$@}", &["${2:-x}"]), "${2:-x}");
}

#[test]
fn text_is_left_alone_unless_a_template_matches() {
    let templates = vec![template("implement", "do $@", None)];
    assert_eq!(super::expand("plain text", &templates), None);
    assert_eq!(super::expand("/other thing", &templates), None);
    assert_eq!(super::expand("/implement", &templates).unwrap(), "do ");
    assert_eq!(
        super::expand("/implement a b", &templates).unwrap(),
        "do a b"
    );
}

#[test]
fn earlier_roots_win_and_later_names_are_reported() {
    let dir = test_dir("shadow");
    let global = dir.join("global");
    let project = dir.join("project");
    write(
        global.join("implement.md"),
        "---\ndescription: global\n---\nfrom global\n",
    );
    write(
        project.join("implement.md"),
        "---\ndescription: project\n---\nfrom project\n",
    );
    write(project.join("extra.md"), "from extra\n");

    let set = load(&[global, project]);
    assert_eq!(set.templates.len(), 2, "{:?}", set.templates);
    let implement = set
        .templates
        .iter()
        .find(|template| template.name == "implement")
        .expect("implement");
    assert_eq!(implement.content, "from global\n");
    assert_eq!(set.warnings.len(), 1);
    assert_eq!(set.warnings[0].kind, WarningKind::Collision);
    assert!(set.warnings[0].message.contains("already loaded from"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn broken_files_warn_and_the_rest_still_load() {
    let dir = test_dir("broken");
    write(
        dir.join("open.md"),
        "---\ndescription: unterminated\nno closing fence\n",
    );
    write(
        dir.join("nested.md"),
        "---\ndescription: ok\n  nested: x\n---\nbody\n",
    );
    write(
        dir.join("good.md"),
        "---\ndescription: fine\nargument-hint: <task>\n---\nbody\n",
    );
    write(dir.join("notes.txt"), "ignored\n");

    let set = load(std::slice::from_ref(&dir));
    let names: Vec<&str> = set.templates.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, vec!["good", "nested"]);
    let good = &set.templates[0];
    assert_eq!(good.argument_hint.as_deref(), Some("<task>"));
    let kinds: Vec<WarningKind> = set.warnings.iter().map(|warning| warning.kind).collect();
    assert_eq!(kinds, vec![WarningKind::Parse, WarningKind::Parse]);
    assert!(
        set.warnings
            .iter()
            .any(|warning| warning.message.contains("not terminated"))
    );
    assert!(
        set.warnings
            .iter()
            .any(|warning| warning.message.contains("unsupported frontmatter"))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn description_defaults_to_the_first_body_line_by_chars() {
    let dir = test_dir("desc");
    let long = "的".repeat(70);
    write(dir.join("plain.md"), &format!("{long}\nrest\n"));
    write(dir.join("short.md"), "one line\n\nmore\n");
    write(dir.join("empty.md"), "\n   \n");

    let set = load(std::slice::from_ref(&dir));
    let find = |name: &str| {
        set.templates
            .iter()
            .find(|template| template.name == name)
            .expect("template")
    };
    let plain = find("plain");
    assert_eq!(plain.description.chars().count(), 63);
    assert!(plain.description.ends_with("..."));
    let short = find("short");
    assert_eq!(short.description, "one line");
    let empty = find("empty");
    assert_eq!(empty.description, "");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn switching_off_defaults_keeps_explicit_paths() {
    let dir = test_dir("flags");
    let explicit = dir.join("mine");
    write(
        explicit.join("flow.md"),
        "---\ndescription: explicit\n---\nbody\n",
    );
    let extra = vec![explicit.to_string_lossy().to_string()];

    let all = template_roots(&dir.join("agent"), &dir, &extra, true, true);
    assert!(all.contains(&dir.join("agent").join("prompts")));
    assert!(all.contains(&dir.join(".timo").join("prompts")));
    assert!(all.contains(&dir.join(".pi").join("prompts")));
    assert_eq!(all.last().unwrap(), &explicit);

    let only_explicit = template_roots(&dir.join("agent"), &dir, &extra, false, true);
    assert_eq!(only_explicit, vec![explicit.clone()]);
    let set = load(&only_explicit);
    assert_eq!(set.templates.len(), 1);
    assert_eq!(set.templates[0].name, "flow");
    let _ = std::fs::remove_dir_all(&dir);
}

fn template(name: &str, content: &str, hint: Option<&str>) -> Template {
    Template {
        name: name.into(),
        description: format!("template {name}"),
        argument_hint: hint.map(str::to_string),
        content: content.into(),
        file_path: PathBuf::from(format!("/prompts/{name}.md")),
    }
}

fn test_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("timo-templates-{tag}-{}", std::process::id()))
}

fn write(path: impl AsRef<Path>, content: &str) {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(path, content).expect("write fixture");
}
