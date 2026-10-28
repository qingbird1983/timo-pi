use super::*;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

fn temp(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("timo-trust-resolve-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

struct Answer(Mutex<Option<TrustChoice>>);

impl ProjectTrustPrompt for Answer {
    fn ask<'a>(
        &'a self,
        _request: TrustRequest,
    ) -> Pin<Box<dyn Future<Output = Option<TrustChoice>> + Send + 'a>> {
        Box::pin(std::future::ready(self.0.lock().unwrap().take()))
    }
}

fn input<'a>(cwd: &'a Path, data: &'a Path, global: &'a timo_config::Settings) -> ResolveInput<'a> {
    ResolveInput {
        cwd,
        data_dir: data,
        global,
        override_decision: None,
        extension: None,
        prompt: None,
        interactive: false,
    }
}

fn touch_resource(cwd: &Path) {
    fs::create_dir_all(cwd.join(".timo")).unwrap();
    fs::write(cwd.join(".timo").join("SYSTEM.md"), "x").unwrap();
}

#[tokio::test]
async fn no_resources_is_trusted_without_prompt() {
    let data = temp("none");
    let work = data.join("work");
    fs::create_dir_all(&work).unwrap();
    let global = timo_config::Settings::default();
    let out = resolve(input(&work, &data, &global)).await;
    assert!(out.trusted);
    assert_eq!(out.source, TrustSource::NoResources);
    let _ = fs::remove_dir_all(&data);
}

#[tokio::test]
async fn override_wins_and_does_not_persist() {
    let data = temp("override");
    let work = data.join("work");
    touch_resource(&work);
    let global = timo_config::Settings::default();
    let mut req = input(&work, &data, &global);
    req.override_decision = Some(true);
    let out = resolve(req).await;
    assert!(out.trusted);
    assert_eq!(out.source, TrustSource::Override);
    assert!(!TrustStore::new(&data).path().exists());
    let _ = fs::remove_dir_all(&data);
}

#[tokio::test]
async fn saved_ancestor_decision_applies() {
    let data = temp("saved");
    let work = data.join("repo").join("crate");
    touch_resource(&work);
    let store = TrustStore::new(&data);
    store.set_many(&[(data.join("repo"), Some(true))]).unwrap();
    let global = timo_config::Settings::default();
    let out = resolve(input(&work, &data, &global)).await;
    assert!(out.trusted);
    assert_eq!(out.source, TrustSource::Store);
    let _ = fs::remove_dir_all(&data);
}

#[tokio::test]
async fn default_always_and_never_apply() {
    let data = temp("default");
    let work = data.join("work");
    touch_resource(&work);
    let global = timo_config::Settings {
        default_project_trust: timo_config::DefaultProjectTrust::Always,
        ..Default::default()
    };
    let out = resolve(input(&work, &data, &global)).await;
    assert!(out.trusted);
    assert_eq!(out.source, TrustSource::Default);

    let global = timo_config::Settings {
        default_project_trust: timo_config::DefaultProjectTrust::Never,
        ..Default::default()
    };
    let out = resolve(input(&work, &data, &global)).await;
    assert!(!out.trusted);
    assert_eq!(out.skipped.len(), 1);
    let _ = fs::remove_dir_all(&data);
}

#[tokio::test]
async fn ask_without_ui_is_rejected() {
    let data = temp("noui");
    let work = data.join("work");
    touch_resource(&work);
    let global = timo_config::Settings::default();
    let out = resolve(input(&work, &data, &global)).await;
    assert!(!out.trusted);
    assert_eq!(out.source, TrustSource::NoUi);
    let _ = fs::remove_dir_all(&data);
}

#[tokio::test]
async fn prompt_choice_is_saved_for_directory() {
    let data = temp("prompt");
    let work = data.join("work");
    touch_resource(&work);
    let global = timo_config::Settings::default();
    let answer = Answer(Mutex::new(Some(TrustChoice {
        trusted: true,
        scope: TrustScopeChoice::Directory,
    })));
    let mut req = input(&work, &data, &global);
    req.prompt = Some(&answer);
    let out = resolve(req).await;
    assert!(out.trusted);
    assert_eq!(out.source, TrustSource::Prompt);
    assert!(
        TrustStore::new(&data)
            .nearest(&work)
            .unwrap()
            .unwrap()
            .decision
    );
    let _ = fs::remove_dir_all(&data);
}

#[tokio::test]
async fn session_choice_does_not_persist() {
    let data = temp("session");
    let work = data.join("work");
    touch_resource(&work);
    let global = timo_config::Settings::default();
    let answer = Answer(Mutex::new(Some(TrustChoice {
        trusted: true,
        scope: TrustScopeChoice::Session,
    })));
    let mut req = input(&work, &data, &global);
    req.prompt = Some(&answer);
    let out = resolve(req).await;
    assert!(out.trusted);
    assert!(!TrustStore::new(&data).path().exists());
    let _ = fs::remove_dir_all(&data);
}

#[test]
fn parent_choice_clears_child_entry() {
    let data = temp("parent");
    let work = data.join("repo").join("crate");
    fs::create_dir_all(&work).unwrap();
    let store = TrustStore::new(&data);
    store.set_many(&[(work.clone(), Some(false))]).unwrap();
    save_choice(
        &store,
        &work,
        TrustChoice {
            trusted: true,
            scope: TrustScopeChoice::Parent,
        },
    )
    .unwrap();
    assert!(store.nearest(&work).unwrap().unwrap().decision);
    assert_eq!(
        store.nearest(&work).unwrap().unwrap().path,
        work.parent().unwrap()
    );
    let _ = fs::remove_dir_all(&data);
}
