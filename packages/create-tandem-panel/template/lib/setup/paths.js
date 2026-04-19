import { homedir } from "os";
import { join, resolve } from "path";

function defaultConfigBase(platform, home, env) {
  if (platform === "darwin") {
    return env.XDG_CONFIG_HOME || join(home, "Library", "Application Support");
  }
  if (platform === "win32") {
    return env.XDG_CONFIG_HOME || env.APPDATA || join(home, "AppData", "Roaming");
  }
  return env.XDG_CONFIG_HOME || join(home, ".config");
}

function defaultDataBase(platform, home, env) {
  if (platform === "darwin") {
    return env.XDG_DATA_HOME || join(home, "Library", "Application Support");
  }
  if (platform === "win32") {
    return (
      env.XDG_DATA_HOME ||
      env.LOCALAPPDATA ||
      env.APPDATA ||
      join(home, "AppData", "Local")
    );
  }
  return env.XDG_DATA_HOME || join(home, ".local", "share");
}

function normalizeDir(value, fallback) {
  const text = String(value || "").trim();
  return resolve(text || fallback);
}

function resolveSetupPaths(options = {}) {
  const env = options.env || process.env;
  const platform = options.platform || process.platform;
  const home = options.home || env.HOME || homedir();
  const allowAmbientStateEnv = options.allowAmbientStateEnv !== false;

  const configBase = normalizeDir(
    env.TANDEM_CONFIG_HOME,
    defaultConfigBase(platform, home, env)
  );
  const dataBase = normalizeDir(env.TANDEM_DATA_HOME, defaultDataBase(platform, home, env));
  const configDir = resolve(configBase, "tandem");
  const dataDir = resolve(dataBase, "tandem");
  const logsDir = resolve(dataDir, "logs");
  const controlPanelStateDir = normalizeDir(
    allowAmbientStateEnv ? env.TANDEM_CONTROL_PANEL_STATE_DIR : "",
    resolve(dataDir, "control-panel")
  );
  const engineStateDir = normalizeDir(
    allowAmbientStateEnv ? env.TANDEM_STATE_DIR : "",
    resolve(dataDir, "data")
  );
  const envFile = normalizeDir(
    env.TANDEM_CONTROL_PANEL_ENV_FILE,
    resolve(configDir, "control-panel.env")
  );
  const tokenFile = resolve(dataDir, "security", "engine_api_token");

  return {
    home: resolve(home),
    configBase,
    dataBase,
    configDir,
    dataDir,
    logsDir,
    controlPanelStateDir,
    engineStateDir,
    envFile,
    tokenFile,
  };
}

export { resolveSetupPaths };
