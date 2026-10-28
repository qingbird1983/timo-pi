//! CLI 参数与选项映射测试（生产入口 `build_options` 的回归面）。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

/// 用真实 clap 解析器构造 `Common`（测试镜像生产路径，不手搭结构体）。
fn common(args: &[&str]) -> Common {
    let cmd =
        <Common as clap::Args>::augment_args(clap::Command::new("timo").disable_help_flag(true));
    // argv[0] 是程序名：不补一位的话第一个 flag 会被当成命令名吃掉。
    let mut argv = vec!["timo"];
    argv.extend_from_slice(args);
    let matches = cmd.try_get_matches_from(argv).expect("parse");
    <Common as clap::FromArgMatches>::from_arg_matches(&matches).expect("into Common")
}

#[test]
fn trust_flags_map_to_override_and_conflict() {
    assert_eq!(
        build_options(&common(&["--approve"]), None, Some(false))
            .expect("options")
            .trust_override,
        Some(true)
    );
    assert_eq!(
        build_options(&common(&["--no-approve"]), None, Some(false))
            .expect("options")
            .trust_override,
        Some(false)
    );
    assert_eq!(
        build_options(&common(&[]), None, None)
            .expect("options")
            .trust_override,
        None
    );

    let cmd =
        <Common as clap::Args>::augment_args(clap::Command::new("timo").disable_help_flag(true));
    assert!(
        cmd.try_get_matches_from(["timo", "--approve", "--no-approve"])
            .is_err()
    );
}

#[test]
fn tool_allowlist_and_prompt_flags_reach_kernel_options() {
    let options = build_options(
        &common(&[
            "--no-session",
            "--tools",
            "read, grep",
            "--append-system-prompt",
            "flag rule",
        ]),
        None,
        None,
    )
    .expect("options");
    assert!(options.no_session);
    assert_eq!(
        options.tool_allowlist.as_deref(),
        Some(["read".to_string(), "grep".to_string()].as_slice())
    );
    assert_eq!(options.append_system_prompt, ["flag rule".to_string()]);
}

#[test]
fn at_prefix_reads_the_file_into_the_prompt_appendix() {
    let dir = std::env::temp_dir().join(format!("timo-cli-append-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("rule.md");
    std::fs::write(&path, "rule from file").expect("write");
    let flag = format!("@{}", path.display());
    let options = build_options(&common(&["--append-system-prompt", &flag]), None, None)
        .expect("readable file");
    assert_eq!(options.append_system_prompt, ["rule from file".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 走生产入口拿用法错误文本（`KernelOptions` 不含 Debug，不能 unwrap_err）。
fn usage_error(common: &Common) -> String {
    match build_options(common, None, None) {
        Ok(_) => panic!("expected a usage error"),
        Err(error) => error,
    }
}

#[test]
fn empty_tool_list_and_unreadable_append_file_are_usage_errors() {
    assert_eq!(
        usage_error(&common(&["--tools", ",,"])),
        "--tools expects at least one tool name or pattern"
    );
    let error = usage_error(&common(&["--append-system-prompt", "@missing-rule.md"]));
    assert!(
        error.starts_with("--append-system-prompt cannot read `missing-rule.md`:"),
        "{error}"
    );
}

#[test]
fn a_bare_model_flag_leaves_the_provider_empty() {
    let options = build_options(&common(&["--model", "deepseek-chat"]), None, None)
        .expect("bare --model is legal");
    assert_eq!(
        options
            .model_override
            .as_ref()
            .map(|(p, m)| (p.as_str(), m.as_str())),
        Some(("", "deepseek-chat"))
    );
    assert_eq!(
        usage_error(&common(&["--provider", "deepseek"])),
        "--provider needs --model as well"
    );
}

#[test]
fn template_flags_reach_kernel_options() {
    let options = build_options(
        &common(&[
            "--prompt-template",
            "a.md",
            "--prompt-template",
            " b ",
            "--no-prompt-templates",
        ]),
        None,
        None,
    )
    .expect("options");
    assert_eq!(
        options.prompt_template_paths,
        ["a.md".to_string(), "b".to_string()]
    );
    assert!(options.no_prompt_templates);
    // 空白项被丢掉，不会把 cwd 本身当成模板目录去扫。
    assert_eq!(
        build_options(&common(&["--prompt-template", "   "]), None, None)
            .expect("options")
            .prompt_template_paths,
        Vec::<String>::new()
    );
}
