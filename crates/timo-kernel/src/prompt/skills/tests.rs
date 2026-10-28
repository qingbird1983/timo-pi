use super::*;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    std::fs::write(path, content).expect("write");
}

fn skill_file(name: &str) -> String {
    format!("---\nname: {name}\ndescription: does {name}\n---\nBody of {name}.\n")
}

fn sample(name: &str, hidden: bool) -> Skill {
    Skill {
        name: name.into(),
        description: format!("{name} <desc> & more"),
        file_path: PathBuf::from(format!("/s/{name}/SKILL.md")),
        base_dir: PathBuf::from(format!("/s/{name}")),
        disable_model_invocation: hidden,
    }
}

#[test]
fn skill_root_stops_recursion() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("skills");
    write(&root.join("a/SKILL.md"), &skill_file("a"));
    write(&root.join("a/nested/SKILL.md"), &skill_file("nested"));

    let set = discover(&[root]);
    let names: Vec<&str> = set.skills.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["a"]);
    assert!(set.warnings.is_empty());
}

#[test]
fn standalone_md_needs_a_description() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("skills");
    write(
        &root.join("good.md"),
        "---\nname: good\ndescription: useful\n---\nbody",
    );
    write(&root.join("bad.md"), "---\nname: bad\n---\nbody");

    let set = discover(&[root]);
    let names: Vec<&str> = set.skills.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["good"]);
    assert!(
        set.warnings
            .iter()
            .any(|w| w.kind == WarningKind::InvalidDescription)
    );
}

#[test]
fn invalid_name_warns_but_loads() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("skills");
    write(&root.join("Weird/SKILL.md"), &skill_file("Weird"));

    let set = discover(&[root]);
    assert_eq!(set.skills.len(), 1);
    assert!(
        set.warnings
            .iter()
            .any(|w| w.kind == WarningKind::InvalidName)
    );
}

#[test]
fn collisions_keep_the_first_and_warn() {
    let temp = tempfile::tempdir().expect("tempdir");
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    write(&first.join("dup/SKILL.md"), &skill_file("dup"));
    write(&second.join("dup/SKILL.md"), &skill_file("dup"));

    let set = discover(&[first.clone(), second]);
    assert_eq!(set.skills.len(), 1);
    assert!(set.skills[0].file_path.starts_with(&first));
    assert!(
        set.warnings
            .iter()
            .any(|w| w.kind == WarningKind::Collision)
    );
}

#[test]
fn ignored_directories_are_skipped() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("skills");
    write(&root.join(".ignore"), "hidden-skill/\n");
    write(
        &root.join("hidden-skill/SKILL.md"),
        &skill_file("hidden-skill"),
    );
    write(&root.join("node_modules/pkg/SKILL.md"), &skill_file("pkg"));
    write(&root.join("ok/SKILL.md"), &skill_file("ok"));

    let set = discover(&[root]);
    let names: Vec<&str> = set.skills.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["ok"]);
}

#[test]
fn frontmatter_handles_colons_and_quotes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("skills");
    write(
        &root.join("x/SKILL.md"),
        "---\nname: x\ndescription: \"use when: parsing\"\n---\nbody",
    );

    let set = discover(&[root]);
    assert_eq!(set.skills[0].description, "use when: parsing");
}

#[test]
fn expansion_includes_body_and_arguments() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("skills");
    write(&root.join("pdf/SKILL.md"), &skill_file("pdf"));
    let set = discover(&[root]);

    let text = expand_command("/skill:pdf extract report.pdf", &set.skills).expect("expand");
    assert!(text.starts_with("<skill name=\"pdf\""));
    assert!(text.contains("Body of pdf."));
    assert!(text.ends_with("\n\nextract report.pdf"));
    assert!(expand_command("/skill:nope x", &set.skills).is_none());
}

#[test]
fn hidden_skills_are_excluded() {
    let skills = vec![sample("a", false), sample("b", true)];
    let text = format_for_prompt(&skills, Reader::Read);
    assert!(text.contains("<name>a</name>"));
    assert!(text.contains("<description>a &lt;desc&gt; &amp; more</description>"));
    assert!(!text.contains("<name>b</name>"));
    assert!(format_for_prompt(&skills[1..], Reader::Read).is_empty());
}

#[test]
fn reader_wording_follows_tool() {
    let skills = vec![sample("a", false)];
    assert!(format_for_prompt(&skills, Reader::Read).contains("with the read tool"));
    assert!(format_for_prompt(&skills, Reader::Bash).contains("with bash"));
}
