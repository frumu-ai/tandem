use std::ffi::OsString;
use std::path::PathBuf;

static PROVIDER_AUTH_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct ProviderAuthTestGuard {
    _guard: tokio::sync::MutexGuard<'static, ()>,
    previous_disable_keyring: Option<OsString>,
    previous_tandem_home: Option<OsString>,
    tandem_home: PathBuf,
}

impl Drop for ProviderAuthTestGuard {
    fn drop(&mut self) {
        restore_env_var(
            "TANDEM_PROVIDER_AUTH_DISABLE_KEYRING",
            self.previous_disable_keyring.take(),
        );
        restore_env_var("TANDEM_HOME", self.previous_tandem_home.take());
        let _ = std::fs::remove_dir_all(&self.tandem_home);
    }
}

fn restore_env_var(name: &str, previous: Option<OsString>) {
    if let Some(value) = previous {
        std::env::set_var(name, value);
    } else {
        std::env::remove_var(name);
    }
}

async fn provider_auth_test_guard() -> ProviderAuthTestGuard {
    let guard = PROVIDER_AUTH_TEST_LOCK.lock().await;
    let tandem_home = std::env::temp_dir().join(format!(
        "tandem-runtime-provider-auth-{}",
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&tandem_home).expect("provider auth test home");
    let previous_disable_keyring = std::env::var_os("TANDEM_PROVIDER_AUTH_DISABLE_KEYRING");
    let previous_tandem_home = std::env::var_os("TANDEM_HOME");
    std::env::set_var("TANDEM_PROVIDER_AUTH_DISABLE_KEYRING", "1");
    std::env::set_var("TANDEM_HOME", &tandem_home);
    ProviderAuthTestGuard {
        _guard: guard,
        previous_disable_keyring,
        previous_tandem_home,
        tandem_home,
    }
}
