//! Self-update for the desktop program (`docs/release.md`). Release builds carry their version and the GitHub
//! repository they come from (`owner/name`) in the `TIDEWATER_VERSION` and `TIDEWATER_REPO` environment variables at
//! build time; a build without both is a development build, which reports "dev" and never checks.
//!
//! At start-up a background thread asks GitHub for the latest release of the public repository (no sign-in, one
//! request). If it is newer than this build, the main menu offers "Update now" and "Later". Every failure of the check,
//! including the 404 an unpublished repository gives, is logged at debug level (shown with `TIDEWATER_DEBUG=1`) and otherwise ignored.
//!
//! "Update now" per platform: Windows downloads the Inno Setup installer and starts it silently; macOS downloads the
//! disk image and swaps the running `.app` bundle; Linux run from an AppImage (`$APPIMAGE` is set) downloads the
//! new AppImage, writes it next to the running one and renames it over, then starts it. Linux run from the tar.gz has no
//! download to install (it is unpacked by hand), so it opens the release page. The page opens in the browser wherever the above doesn't apply too
//! (a bundle in a read-only place, a build run from a source tree). On the phones (the Android APK is sideloaded, so no
//! store updates it) the check runs the same way and "Update now" hands the release page to the shell to open. A download's size is checked against the size the release lists; there is no other signing.
//!
//! **Testing without a release.** `TIDEWATER_UPDATE_FEED` replaces GitHub's `releases/latest` answer with a feed of the
//! same shape: an `http(s)://` address or a path to a JSON file. With it set, the download and release-page address
//! checks are off (the feed is trusted), and the program says so on stderr. It needs a build that has a version, i.e.
//! one made with `TIDEWATER_VERSION` and `TIDEWATER_REPO` set (`docs/release.md`).
//!
//! **Where the app can't replace itself.** On macOS an app opened from the disk image, or straight from Downloads while
//! quarantined, runs from a read-only or randomised path (App Translocation). [`needs_move`] spots that; the main menu
//! then shows "Move Tidewater to Applications to enable updates." once, and an update offer opens the release page.
//!
//! The pure parts (versions, the release JSON, asset choice, when the check runs) are tested below; the network and
//! file steps are in [`net`] and only exist on the desktop.

use std::sync::{Arc, Mutex};

use serde::Deserialize;

/// The repository releases are published to (`owner/name`), set by the release build.
fn repo() -> Option<&'static str> {
    option_env!("TIDEWATER_REPO").filter(|r| !r.is_empty())
}

/// The only place a release's page and downloads may be on; anything else in the JSON is ignored.
fn page_prefix(repo: &str) -> String {
    format!("https://github.com/{repo}/")
}

/// The version the program reports (`--version`): the tag's number for a release build, "dev" otherwise.
pub const VERSION: &str = match option_env!("TIDEWATER_VERSION") {
    Some(v) if !v.is_empty() => v,
    _ => "dev",
};

/// This build's version: the tag's number for a release build, `None` for a development build.
pub fn build_version() -> Option<&'static str> {
    repo()?;
    option_env!("TIDEWATER_VERSION").filter(|v| !v.is_empty())
}

/// A version as three numbers. A pre-release suffix (`1.2.3-rc1`) is not accepted: such builds neither check nor are
/// offered.
fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches('v');
    let mut it = s.split('.');
    let mut next = || it.next()?.parse::<u64>().ok();
    let v = (next()?, next()?, next()?);
    it.next().is_none().then_some(v)
}

/// Whether `latest` is a newer version than `current`; either one not being a plain version gives `false`.
pub fn is_newer(current: &str, latest: &str) -> bool {
    matches!((parse_version(current), parse_version(latest)), (Some(c), Some(l)) if l > c)
}

/// Whether the start-up check runs: a release build, not turned off by the flag or the setting, and not a scripted or
/// headless run. Never on the PS Vita, which installs a new `.vpk` by hand.
pub fn enabled(version: Option<&str>, no_flag: bool, setting: bool, scripted: bool) -> bool {
    version.is_some_and(|v| parse_version(v).is_some()) && !no_flag && setting && !scripted && !cfg!(target_os = "vita")
}

/// The systems the program has release downloads for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    /// Linux run from an AppImage: the file to replace is `$APPIMAGE`.
    LinuxAppImage,
}

impl Platform {
    /// This build's system, if it has downloads (x64 Windows, any Mac).
    pub fn current() -> Option<Self> {
        if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            Some(Self::Windows)
        } else if cfg!(target_os = "macos") {
            Some(Self::MacOs)
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) && appimage_path().is_some() {
            Some(Self::LinuxAppImage)
        } else {
            None
        }
    }

    /// The end of the asset name this system installs from (`tidewater-<version>-<this>`). Linux run from the tar.gz
    /// has none: it is unpacked by hand, so it opens the release page.
    fn suffix(self) -> &'static str {
        match self {
            Self::Windows => "-windows-x64-setup.exe",
            Self::MacOs => "-macos.dmg",
            Self::LinuxAppImage => "-linux-x86_64.AppImage",
        }
    }
}

/// The AppImage file the program is running from, if it is: `$APPIMAGE` as the AppImage runtime sets it.
fn appimage_path() -> Option<std::path::PathBuf> {
    appimage_from(std::env::var_os("APPIMAGE").as_deref())
}

/// [`appimage_path`] for a given value of `$APPIMAGE`: only an absolute path to a named file counts, so a stray or
/// relative value never makes the program write somewhere unexpected.
fn appimage_from(value: Option<&std::ffi::OsStr>) -> Option<std::path::PathBuf> {
    let path = std::path::Path::new(value.filter(|v| !v.is_empty())?);
    (path.is_absolute() && path.file_name().is_some()).then(|| path.to_path_buf())
}

/// Where the new AppImage is written while it downloads: beside the file it replaces (so the final rename stays on one
/// file system and is atomic), hidden, and never over a file that is there already.
fn appimage_staging(target: &std::path::Path, pid: u32) -> Option<std::path::PathBuf> {
    let name = target.file_name()?.to_string_lossy();
    Some(target.with_file_name(format!(".{name}.update-{pid}")))
}

/// A release's download for this system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

/// A release newer than this build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// The version without the `v`.
    pub version: String,
    pub page: String,
    /// The download for this system, if the release has one.
    pub asset: Option<Asset>,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

/// Reads `repo`'s `releases/latest` answer: the release, with the asset for `platform`, if its version is newer than
/// `current`. `None` for anything else, including JSON that isn't one.
pub fn parse_latest(json: &str, repo: &str, current: &str, platform: Option<Platform>) -> Option<Release> {
    parse_feed(json, &page_prefix(repo), current, platform)
}

/// [`parse_latest`] with the address every link must start with given directly (empty: any, for a test feed).
fn parse_feed(json: &str, prefix: &str, current: &str, platform: Option<Platform>) -> Option<Release> {
    let r: ApiRelease = serde_json::from_str(json).ok()?;
    if r.draft || r.prerelease || !is_newer(current, &r.tag_name) || !r.html_url.starts_with(prefix) {
        return None;
    }
    let version = r.tag_name.trim_start_matches('v').to_string();
    let asset = platform.and_then(|p| {
        r.assets
            .into_iter()
            .find(|a| a.name.starts_with("tidewater-") && a.name.ends_with(p.suffix()))
            .filter(|a| a.browser_download_url.starts_with(prefix) && a.size > 0)
            .map(|a| Asset {
                name: a.name,
                url: a.browser_download_url,
                size: a.size,
            })
    });
    Some(Release {
        version,
        page: r.html_url,
        asset,
    })
}

/// Whether a macOS app bundle at `bundle` can't be replaced by an update: it runs from the disk image (`/Volumes`),
/// from App Translocation's randomised read-only copy, or from a folder the program can't write to (`writable`).
pub fn needs_move(bundle: &std::path::Path, writable: bool) -> bool {
    let s = bundle.to_string_lossy();
    !writable || s.contains("/AppTranslocation/") || s.starts_with("/Volumes/")
}

/// What the main menu shows about the update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    None,
    /// A newer version (its number) is out: "Update now" and "Later".
    Offer(String),
    /// "Update now" was chosen and the download or the swap is running.
    Busy,
    /// macOS: the app runs from a place it can't update itself in; the line says to move it to Applications.
    MoveToApplications,
}

enum State {
    Idle,
    Offer(Release),
    Busy,
    /// The app can't update itself where it is (see [`needs_move`]); nothing else is going on. macOS only.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    MoveHint,
    /// The update is installed (or its installer started): the program should end. Desktop only: a phone only opens the
    /// release page.
    #[cfg_attr(any(target_os = "ios", target_os = "android"), allow(dead_code))]
    Finished,
}

/// The update check and install, shared between the core and the thread that does the work.
#[derive(Clone)]
pub struct Updater(Arc<Mutex<State>>);

impl Default for Updater {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(State::Idle)))
    }
}

impl Updater {
    fn with<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        f(&mut self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Starts the check in the background if [`enabled`] says so.
    pub fn start_check(&self, no_flag: bool, setting: bool, scripted: bool) {
        if !enabled(build_version(), no_flag, setting, scripted) {
            return;
        }
        {
            let this = self.clone();
            let current = build_version().unwrap_or_default().to_string();
            let spawned =
                std::thread::Builder::new()
                    .name("update-check".into())
                    .spawn(move || match net::latest(&current) {
                        Ok(Some(r)) => this.with(|s| *s = State::Offer(r)),
                        Ok(None) => debug("up to date"),
                        Err(e) => debug(&format!("check failed: {e:#}")),
                    });
            if let Err(e) = spawned {
                debug(&format!("no thread for the check: {e}"));
            }
        }
    }

    /// macOS: if the app runs from a place it can't update itself in (the disk image, App Translocation, a read-only
    /// folder) and the player hasn't been told yet (`seen`), the menu shows the move hint until the check or the player
    /// replaces it. Does nothing elsewhere, in a development build or when updates are off.
    pub fn check_location(&self, seen: bool, enabled: bool) {
        #[cfg(target_os = "macos")]
        if enabled && !seen && build_version().is_some() && net::bundle_blocks_update() {
            self.with(|s| {
                if matches!(s, State::Idle) {
                    *s = State::MoveHint;
                }
            });
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (seen, enabled, self);
    }

    pub fn notice(&self) -> Notice {
        self.with(|s| match s {
            State::Offer(r) => Notice::Offer(r.version.clone()),
            State::Busy => Notice::Busy,
            State::MoveHint => Notice::MoveToApplications,
            State::Idle | State::Finished => Notice::None,
        })
    }

    /// "Later": hides the offer until the next start.
    pub fn dismiss(&self) {
        self.with(|s| {
            if matches!(s, State::Offer(_) | State::MoveHint) {
                *s = State::Idle;
            }
        });
    }

    /// The update is in place and the program should end now.
    pub fn finished(&self) -> bool {
        self.with(|s| matches!(s, State::Finished))
    }

    /// "Update now": installs in the background, or opens the release page where the system can't install by itself
    /// (or when the install fails). On the phones it returns the release page for the shell to open (`SDL_OpenURL`), as
    /// the program can't start a browser there itself.
    #[must_use]
    pub fn install(&self) -> Option<String> {
        let release = self.with(|s| match std::mem::replace(s, State::Busy) {
            State::Offer(r) => Some(r),
            other => {
                *s = other;
                None
            }
        })?;
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            let this = self.clone();
            let page = release.page.clone();
            let spawned = std::thread::Builder::new()
                .name("update-install".into())
                .spawn(move || match net::install(&release) {
                    Ok(true) => this.with(|s| *s = State::Finished),
                    Ok(false) => {
                        net::open_page(&release.page);
                        this.with(|s| *s = State::Idle);
                    }
                    Err(e) => {
                        eprintln!("tidewater: the update failed ({e:#}); opening the release page");
                        net::open_page(&release.page);
                        this.with(|s| *s = State::Idle);
                    }
                });
            if spawned.is_err() {
                net::open_page(&page);
                self.with(|s| *s = State::Idle);
            }
            None
        }
        #[cfg(any(target_os = "ios", target_os = "android"))]
        {
            self.with(|s| *s = State::Idle);
            net::allowed(&release.page).then_some(release.page)
        }
    }
}

/// Debug-level logging: shown only with `TIDEWATER_DEBUG` set, as the check must stay quiet.
fn debug(msg: &str) {
    if std::env::var_os("TIDEWATER_DEBUG").is_some() {
        eprintln!("tidewater: update: {msg}");
    }
}

// The phones use only the check and the page address; the downloads and installs are the desktop's.
#[cfg_attr(any(target_os = "ios", target_os = "android"), allow(dead_code))]
mod net {
    use std::io::{Read, Write};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use anyhow::{Context, Result, bail};

    use super::{Platform, Release, debug, page_prefix, parse_feed, repo};

    const USER_AGENT: &str = concat!("tidewater/", env!("CARGO_PKG_VERSION"));

    /// The test feed (`TIDEWATER_UPDATE_FEED`), if one is set. Said once on stderr so it is never in use unnoticed.
    fn feed() -> Option<String> {
        static FEED: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        FEED.get_or_init(|| {
            let f = std::env::var("TIDEWATER_UPDATE_FEED").ok().filter(|f| !f.is_empty());
            if let Some(f) = &f {
                eprintln!("tidewater: update: using the test feed {f}; address checks are off");
            }
            f
        })
        .clone()
    }

    /// Whether `url` may be downloaded or opened: on the release's repository, or anything while a test feed is set.
    pub(super) fn allowed(url: &str) -> bool {
        feed().is_some() || repo().is_some_and(|r| url.starts_with(&page_prefix(r)))
    }

    fn agent(timeout: Duration) -> ureq::Agent {
        // GitHub answers a release download with a redirect to its file storage; ureq follows up to 10 of them.
        ureq::Agent::config_builder()
            .max_redirects(10)
            .timeout_global(Some(timeout))
            .user_agent(USER_AGENT)
            .build()
            .into()
    }

    /// Asks for the latest release; `Some` if it is newer than `current`.
    pub(super) fn latest(current: &str) -> Result<Option<Release>> {
        let Some(repo) = repo() else { return Ok(None) };
        let (url, prefix) = match feed() {
            Some(f) => (f, String::new()),
            None => (
                format!("https://api.github.com/repos/{repo}/releases/latest"),
                page_prefix(repo),
            ),
        };
        let body = if url.starts_with("http://") || url.starts_with("https://") {
            let mut resp = agent(Duration::from_secs(10))
                .get(&url)
                .header("Accept", "application/vnd.github+json")
                .call()?;
            resp.body_mut().with_config().limit(1 << 20).read_to_string()?
        } else {
            std::fs::read_to_string(&url).with_context(|| format!("reading the feed {url}"))?
        };
        Ok(parse_feed(&body, &prefix, current, Platform::current()))
    }

    /// Installs the release. `Ok(true)`: done, the program should end (the installer runs or the new copy starts).
    /// `Ok(false)`: nothing to install from here, show the release page.
    pub(super) fn install(r: &Release) -> Result<bool> {
        let (Some(asset), Some(platform)) = (&r.asset, Platform::current()) else {
            return Ok(false);
        };
        match platform {
            Platform::Windows => {
                let dir = fresh_temp_dir()?;
                let setup = dir.join(&asset.name);
                download(&asset.url, asset.size, &setup)?;
                std::process::Command::new(&setup)
                    .arg("/SILENT")
                    .spawn()
                    .context("starting the installer")?;
                Ok(true)
            }
            Platform::MacOs => install_mac(&asset.url, asset.size, &asset.name),
            Platform::LinuxAppImage => install_appimage(&asset.url, asset.size),
        }
    }

    fn fresh_temp_dir() -> Result<PathBuf> {
        let dir = std::env::temp_dir().join(format!("tidewater-update-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Downloads `url` to `dest`, failing unless it is exactly `size` bytes.
    fn download(url: &str, size: u64, dest: &Path) -> Result<()> {
        if !allowed(url) {
            bail!("unexpected download address");
        }
        fetch(url, size, dest)
    }

    /// [`download`] without the address check.
    pub(super) fn fetch(url: &str, size: u64, dest: &Path) -> Result<()> {
        let mut resp = agent(Duration::from_secs(30 * 60)).get(url).call()?;
        let mut reader = resp.body_mut().with_config().limit(size + 1).reader();
        let mut file = std::fs::File::create(dest)?;
        let got = std::io::copy(&mut reader.by_ref().take(size + 1), &mut file)?;
        file.flush()?;
        if got != size {
            bail!("the download is {got} bytes, the release says {size}");
        }
        debug(&format!("downloaded {got} bytes"));
        Ok(())
    }

    fn run(cmd: &mut std::process::Command) -> Result<()> {
        let status = cmd
            .status()
            .with_context(|| format!("running {:?}", cmd.get_program()))?;
        if !status.success() {
            bail!("{:?} failed: {status}", cmd.get_program());
        }
        Ok(())
    }

    /// Whether the running `.app` can't be replaced where it is (`super::needs_move`). False when not in a bundle.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(super) fn bundle_blocks_update() -> bool {
        let Some(bundle) = running_bundle() else { return false };
        let probe = bundle
            .parent()
            .map(|p| p.join(format!(".tidewater-probe-{}", std::process::id())));
        let writable = probe.is_some_and(|p| std::fs::create_dir(&p).is_ok() && std::fs::remove_dir(&p).is_ok());
        super::needs_move(&bundle, writable)
    }

    /// The `.app` bundle the running program is in, if it runs from one.
    fn running_bundle() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
        exe.ancestors()
            .find(|p| p.extension().is_some_and(|e| e == "app"))
            .map(Path::to_path_buf)
    }

    /// Downloads the disk image, copies its `.app` beside the running one, swaps them and starts the new copy. A
    /// bundle that can't be replaced (not running from one, a folder we can't write to) is `Ok(false)`.
    fn install_mac(url: &str, size: u64, name: &str) -> Result<bool> {
        let Some(bundle) = running_bundle() else {
            return Ok(false);
        };
        let parent = bundle.parent().context("the bundle has no folder")?;
        let new = parent.join(".Tidewater-update.app");
        let old = parent.join(".Tidewater-old.app");
        let _ = std::fs::remove_dir_all(&new);
        let _ = std::fs::remove_dir_all(&old);
        if std::fs::create_dir(&new).is_err() {
            return Ok(false);
        }
        std::fs::remove_dir(&new)?;

        let dir = fresh_temp_dir()?;
        let dmg = dir.join(name);
        download(url, size, &dmg)?;
        let mount = dir.join("mount");
        std::fs::create_dir(&mount)?;
        run(std::process::Command::new("hdiutil")
            .args(["attach", "-nobrowse", "-readonly", "-noverify", "-mountpoint"])
            .arg(&mount)
            .arg(&dmg))?;
        let copied = run(std::process::Command::new("ditto")
            .arg(mount.join("Tidewater.app"))
            .arg(&new));
        // Detach whatever happened above; a failure to detach doesn't matter to the update.
        let _ = std::process::Command::new("hdiutil")
            .args(["detach", "-quiet", "-force"])
            .arg(&mount)
            .status();
        copied?;
        // The old copy steps aside (a directory can't be renamed over a full one), the new one takes its place.
        std::fs::rename(&bundle, &old)?;
        if let Err(e) = std::fs::rename(&new, &bundle) {
            let _ = std::fs::rename(&old, &bundle);
            return Err(e.into());
        }
        let _ = std::fs::remove_dir_all(&old);
        let _ = std::fs::remove_dir_all(&dir);
        // Start the new copy a moment after this one has ended, so the settings written at exit are the ones it reads.
        std::process::Command::new("sh")
            .args(["-c", "sleep 1; exec open -n \"$0\""])
            .arg(&bundle)
            .spawn()?;
        Ok(true)
    }

    /// Downloads the new AppImage next to the running one, makes it executable, renames it over the running file and
    /// starts it. Running from a folder we can't write to is `Ok(false)` (the release page opens instead).
    #[cfg(unix)]
    fn install_appimage(url: &str, size: u64) -> Result<bool> {
        let Some(target) = super::appimage_path() else {
            return Ok(false);
        };
        if !allowed(url) {
            bail!("unexpected download address");
        }
        if !replace_appimage(&target, url, size)? {
            return Ok(false);
        }
        // Start the new copy a moment after this one has ended, so the settings written at exit are the ones it reads.
        std::process::Command::new("sh")
            .args(["-c", "sleep 1; exec \"$0\""])
            .arg(&target)
            .spawn()
            .context("starting the new version")?;
        Ok(true)
    }

    /// The file steps of [`install_appimage`]: download beside `target`, check, make executable, rename over it.
    /// `Ok(false)` if the folder can't be written to; on any failure the half-written file is removed.
    #[cfg(unix)]
    pub(super) fn replace_appimage(target: &Path, url: &str, size: u64) -> Result<bool> {
        use std::os::unix::fs::PermissionsExt;

        let Some(staging) = super::appimage_staging(target, std::process::id()) else {
            return Ok(false);
        };
        // Probe for write access with the file itself: the folder may be read-only (a mounted share).
        if std::fs::File::create(&staging).is_err() {
            return Ok(false);
        }
        let installed = (|| -> Result<()> {
            fetch(url, size, &staging)?;
            let mut magic = [0u8; 4];
            std::fs::File::open(&staging)?.read_exact(&mut magic)?;
            if &magic != b"\x7fELF" {
                bail!("the download is not a program");
            }
            std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755))?;
            // A rename over the running file is safe on Linux: the running copy keeps its old contents open.
            std::fs::rename(&staging, target)?;
            Ok(())
        })();
        if let Err(e) = installed {
            let _ = std::fs::remove_file(&staging);
            return Err(e);
        }
        Ok(true)
    }

    #[cfg(not(unix))]
    fn install_appimage(_url: &str, _size: u64) -> Result<bool> {
        Ok(false)
    }

    /// Opens a web page in the browser (only ever the release page, whose address was checked).
    pub(super) fn open_page(url: &str) {
        if !allowed(url) {
            return;
        }
        let mut cmd = if cfg!(target_os = "windows") {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", "start", ""]);
            c
        } else if cfg!(target_os = "macos") {
            std::process::Command::new("open")
        } else {
            std::process::Command::new("xdg-open")
        };
        if let Err(e) = cmd.arg(url).spawn() {
            debug(&format!("could not open the release page: {e}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPO: &str = "owner/tidewater";

    const SAMPLE: &str = r#"{
      "tag_name": "v1.4.0",
      "html_url": "https://github.com/owner/tidewater/releases/tag/v1.4.0",
      "draft": false,
      "prerelease": false,
      "assets": [
        {"name": "tidewater-1.4.0-linux-x64.tar.gz", "size": 11,
         "browser_download_url": "https://github.com/owner/tidewater/releases/download/v1.4.0/tidewater-1.4.0-linux-x64.tar.gz"},
        {"name": "tidewater-1.4.0-macos.dmg", "size": 22,
         "browser_download_url": "https://github.com/owner/tidewater/releases/download/v1.4.0/tidewater-1.4.0-macos.dmg"},
        {"name": "tidewater-1.4.0-windows-x64-setup.exe", "size": 33,
         "browser_download_url": "https://github.com/owner/tidewater/releases/download/v1.4.0/tidewater-1.4.0-windows-x64-setup.exe"},
        {"name": "tidewater-1.4.0-linux-x86_64.AppImage", "size": 44,
         "browser_download_url": "https://github.com/owner/tidewater/releases/download/v1.4.0/tidewater-1.4.0-linux-x86_64.AppImage"}
      ]
    }"#;

    #[test]
    fn versions_compare_by_number_not_text() {
        assert!(is_newer("1.2.3", "v1.2.4"));
        assert!(is_newer("1.9.0", "1.10.0"));
        assert!(is_newer("0.9.9", "1.0.0"));
        assert!(!is_newer("1.2.3", "1.2.3"));
        assert!(!is_newer("1.3.0", "1.2.9"));
        // Not plain versions: never newer, never checked against.
        assert!(!is_newer("dev", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.1.0-rc1"));
        assert!(!is_newer("1.0.0", "1.1"));
        assert!(!is_newer("1.0.0", ""));
    }

    #[test]
    fn the_right_asset_is_chosen_per_system() {
        let w = parse_latest(SAMPLE, REPO, "1.3.0", Some(Platform::Windows)).unwrap();
        assert_eq!(w.version, "1.4.0");
        let a = w.asset.unwrap();
        assert_eq!((a.name.as_str(), a.size), ("tidewater-1.4.0-windows-x64-setup.exe", 33));
        let m = parse_latest(SAMPLE, REPO, "1.3.0", Some(Platform::MacOs)).unwrap();
        assert_eq!(m.asset.unwrap().size, 22);
        let l = parse_latest(SAMPLE, REPO, "1.3.0", Some(Platform::LinuxAppImage)).unwrap();
        let a = l.asset.unwrap();
        assert_eq!((a.name.as_str(), a.size), ("tidewater-1.4.0-linux-x86_64.AppImage", 44));
        // Linux from the tarball (and any other system) has no download to install: the release page is all there is.
        let none = parse_latest(SAMPLE, REPO, "1.3.0", None).unwrap();
        assert_eq!(none.asset, None);
        assert_eq!(none.page, "https://github.com/owner/tidewater/releases/tag/v1.4.0");
    }

    #[test]
    fn the_appimage_is_found_only_from_a_usable_path() {
        use std::ffi::OsStr;
        use std::path::Path;
        let ok = appimage_from(Some(OsStr::new("/opt/games/Tidewater.AppImage")));
        assert_eq!(ok.as_deref(), Some(Path::new("/opt/games/Tidewater.AppImage")));
        assert_eq!(appimage_from(None), None);
        assert_eq!(appimage_from(Some(OsStr::new(""))), None);
        assert_eq!(appimage_from(Some(OsStr::new("Tidewater.AppImage"))), None);
        assert_eq!(appimage_from(Some(OsStr::new("/"))), None);
    }

    #[test]
    fn the_new_appimage_is_staged_beside_the_old_one() {
        use std::path::Path;
        let target = Path::new("/opt/games/Tidewater.AppImage");
        let staged = appimage_staging(target, 77).unwrap();
        assert_eq!(staged.parent(), target.parent());
        assert_eq!(staged.file_name().unwrap(), ".Tidewater.AppImage.update-77");
        assert_ne!(staged, target);
    }

    #[test]
    fn the_tarball_and_the_appimage_are_told_apart_by_name() {
        // The AppImage asset is never picked for the tarball's listing, nor the tarball for the AppImage.
        let only_tar = SAMPLE.replace("linux-x86_64.AppImage", "linux-x64.tar.gz");
        let r = parse_latest(&only_tar, REPO, "1.3.0", Some(Platform::LinuxAppImage)).unwrap();
        assert_eq!(r.asset, None);
        let w = parse_latest(SAMPLE, REPO, "1.3.0", Some(Platform::Windows)).unwrap();
        assert_eq!(w.asset.unwrap().size, 33);
    }

    #[test]
    fn only_a_newer_published_release_counts() {
        assert_eq!(parse_latest(SAMPLE, REPO, "1.4.0", Some(Platform::MacOs)), None);
        assert_eq!(parse_latest(SAMPLE, REPO, "2.0.0", Some(Platform::MacOs)), None);
        let pre = SAMPLE.replace("\"prerelease\": false", "\"prerelease\": true");
        assert_eq!(parse_latest(&pre, REPO, "1.0.0", Some(Platform::MacOs)), None);
        let elsewhere = SAMPLE.replace("github.com/owner", "example.com/other");
        let r = parse_latest(&elsewhere, REPO, "1.0.0", Some(Platform::MacOs));
        assert_eq!(r, None);
        assert_eq!(parse_latest("not json", REPO, "1.0.0", None), None);
        assert_eq!(parse_latest(r#"{"message":"Not Found"}"#, REPO, "1.0.0", None), None);
    }

    #[test]
    fn a_download_from_another_place_is_not_offered() {
        let odd = SAMPLE.replace(
            "https://github.com/owner/tidewater/releases/download/v1.4.0/tidewater-1.4.0-macos.dmg",
            "https://example.com/tidewater-1.4.0-macos.dmg",
        );
        let r = parse_latest(&odd, REPO, "1.0.0", Some(Platform::MacOs)).unwrap();
        assert_eq!(r.asset, None);
    }

    #[test]
    fn the_check_runs_only_where_it_should() {
        assert!(enabled(Some("1.0.0"), false, true, false));
        // A development build, the flag, the setting and a scripted or headless run each turn it off.
        assert!(!enabled(None, false, true, false));
        assert!(!enabled(Some("dev"), false, true, false));
        assert!(!enabled(Some("1.0.0"), true, true, false));
        assert!(!enabled(Some("1.0.0"), false, false, false));
        assert!(!enabled(Some("1.0.0"), false, true, true));
    }

    #[test]
    fn a_test_build_never_checks() {
        // The tests are built without TIDEWATER_VERSION or TIDEWATER_REPO.
        assert_eq!(build_version(), None);
        let u = Updater::default();
        u.start_check(false, true, false);
        assert_eq!(u.notice(), Notice::None);
    }

    #[test]
    fn a_feed_without_the_repository_prefix_is_read_with_an_empty_one() {
        let local = SAMPLE.replace("https://github.com/owner/tidewater/", "http://127.0.0.1:8000/");
        // With the repository's prefix a release page elsewhere is refused outright.
        assert_eq!(parse_latest(&local, REPO, "1.0.0", Some(Platform::MacOs)), None);
        let r = parse_feed(&local, "", "1.0.0", Some(Platform::MacOs)).unwrap();
        assert_eq!(
            r.asset.unwrap().url,
            "http://127.0.0.1:8000/releases/download/v1.4.0/tidewater-1.4.0-macos.dmg"
        );
    }

    #[test]
    fn an_app_that_cannot_be_replaced_is_spotted() {
        use std::path::Path;
        let ok = Path::new("/Applications/Tidewater.app");
        assert!(!needs_move(ok, true));
        assert!(needs_move(ok, false));
        assert!(needs_move(Path::new("/Volumes/Tidewater/Tidewater.app"), true));
        let moved = Path::new("/private/var/folders/ab/xyz/T/AppTranslocation/1234-ABCD/d/Tidewater.app");
        assert!(needs_move(moved, true));
        assert!(!needs_move(Path::new("/Applications/Games/Tidewater.app"), true));
    }

    #[test]
    fn a_download_follows_redirects_and_checks_the_size() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let body = vec![7u8; 5000];
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let served = body.clone();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut c, _) = listener.accept().unwrap();
                let mut buf = [0u8; 2048];
                let n = c.read(&mut buf).unwrap();
                let head = String::from_utf8_lossy(&buf[..n]);
                if head.starts_with("GET /release/file ") {
                    let loc = format!("http://127.0.0.1:{port}/storage/file");
                    write!(
                        c,
                        "HTTP/1.1 302 Found\r\nLocation: {loc}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .unwrap();
                } else {
                    write!(
                        c,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        served.len()
                    )
                    .unwrap();
                    c.write_all(&served).unwrap();
                }
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f");
        net::fetch(&format!("http://127.0.0.1:{port}/release/file"), 5000, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        server.join().unwrap();
    }

    #[test]
    fn a_download_of_the_wrong_size_fails() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut c, _) = listener.accept().unwrap();
            let mut buf = [0u8; 2048];
            let _ = c.read(&mut buf).unwrap();
            write!(
                c,
                "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789"
            )
            .unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let err = net::fetch(&format!("http://127.0.0.1:{port}/f"), 9, &dir.path().join("f"));
        assert!(err.is_err());
        server.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn an_appimage_update_replaces_the_file_atomically_and_only_with_a_program() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::os::unix::fs::PermissionsExt;
        let mut elf = b"\x7fELF".to_vec();
        elf.extend_from_slice(&[1u8; 100]);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{port}/f");
        let served = elf.clone();
        let server = std::thread::spawn(move || {
            // The first request gets a program, the second a page of text of the same size.
            for i in 0..2 {
                let (mut c, _) = listener.accept().unwrap();
                let mut buf = [0u8; 2048];
                let _ = c.read(&mut buf).unwrap();
                let body = if i == 0 {
                    served.clone()
                } else {
                    vec![b'x'; served.len()]
                };
                write!(
                    c,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                c.write_all(&body).unwrap();
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("Tidewater.AppImage");
        std::fs::write(&target, b"old").unwrap();

        assert!(net::replace_appimage(&target, &url, elf.len() as u64).unwrap());
        assert_eq!(std::fs::read(&target).unwrap(), elf);
        assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o755);

        // Not a program: refused, the installed file stays, and no staging file is left behind.
        assert!(net::replace_appimage(&target, &url, elf.len() as u64).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), elf);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        server.join().unwrap();

        // A folder that can't be written to is not an error: the release page opens instead.
        let missing = dir.path().join("nowhere").join("Tidewater.AppImage");
        assert!(!net::replace_appimage(&missing, &url, 1).unwrap());
    }

    #[test]
    fn later_hides_the_offer() {
        let u = Updater::default();
        u.with(|s| {
            *s = State::Offer(Release {
                version: "2.0.0".into(),
                page: String::new(),
                asset: None,
            })
        });
        assert_eq!(u.notice(), Notice::Offer("2.0.0".into()));
        u.dismiss();
        assert_eq!(u.notice(), Notice::None);
    }
}
