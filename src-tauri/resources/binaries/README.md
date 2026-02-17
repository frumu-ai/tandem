This directory is intentionally tracked so Tauri resource resolution does not fail
in CI when no bundled engine binary is present.

Release builds populate this folder with the platform-specific `tandem-engine`
binary before packaging.
