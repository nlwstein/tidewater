//! `twtool replay` and `twtool desync-diff`: running recorded games (`.twrep`, `tidewater_replay`) without the client.

use std::path::Path;

use anyhow::{Context, Result, bail};
use tidewater_formats::identity::DataIdentity;
use tidewater_formats::install::Install;
use tidewater_replay::run::{first_desync, verify};
use tidewater_replay::start::{new_world, new_world_unchecked};
use tidewater_replay::{Header, Replay, ReplayError};
use tidewater_sim::World;

/// Reads and checks a replay file.
fn read_replay(path: &Path) -> Result<Replay> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Replay::decode(&bytes).with_context(|| format!("{}", path.display()))
}

/// The map a replay was recorded on: the file given with `--map`, else the recorded name as a file on disk or inside
/// the install's archives. Whether it is the right file is `new_world`'s check.
fn map_bytes(install: &mut Install, header: &Header, over: Option<&Path>) -> Result<Vec<u8>> {
    if let Some(p) = over {
        return std::fs::read(p).with_context(|| format!("reading {}", p.display()));
    }
    if Path::new(&header.map_name).is_file() {
        return Ok(std::fs::read(&header.map_name)?);
    }
    install.read(&header.map_name).with_context(|| {
        format!(
            "finding the map {} the replay was recorded on (give it with --map)",
            header.map_name
        )
    })
}

/// The world a replay starts from, with the map checked against the recording's hash.
fn start(install: &mut Install, header: &Header, over: Option<&Path>) -> Result<World> {
    let bytes = map_bytes(install, header, over)?;
    new_world(install, &bytes, header)
}

/// `twtool replay`: plays the replay headlessly and checks every checkpoint.
pub fn replay(
    file: &Path,
    install: &Path,
    map: Option<&Path>,
    force: bool,
    mods: Option<(&Path, &[String])>,
) -> Result<()> {
    let r = read_replay(file)?;
    let mut inst = Install::open(install)?;
    if let Some((dir, names)) = mods {
        inst = inst.with_mods(std::sync::Arc::new(tidewater_formats::mods::ModSet::resolve(
            dir, names,
        )?));
    }
    println!(
        "{}: map {}, seed {}, {}, {} cycles, {} checkpoints, recorded by {}",
        file.display(),
        r.header.map_name,
        r.header.seed,
        r.header.rules,
        r.cycles,
        r.checkpoints.len(),
        r.header.sim_build
    );
    if let Some(d) = r.header.data.as_ref().filter(|d| !d.mods.is_empty()) {
        println!("mods: {}", tidewater_formats::identity::describe_mods(&d.mods));
    }
    for n in &r.notes {
        println!("note: {n}");
    }
    let mut world = if force {
        if let Some(d) = &r.header.data
            && let Some(m) = d.mismatch(&DataIdentity::of(&mut inst)?)
        {
            println!("warning: recorded with different game files: {m}; playing anyway (--force)");
        }
        let bytes = map_bytes(&mut inst, &r.header, map)?;
        new_world_unchecked(&mut inst, &bytes, &r.header)?
    } else {
        start(&mut inst, &r.header, map).map_err(|e| match e.downcast_ref::<ReplayError>() {
            Some(ReplayError::Data(_)) => e.context("use --force to play it anyway"),
            _ => e,
        })?
    };
    match verify(&mut world, &r) {
        Ok(rep) => {
            println!(
                "ok: {} cycles, all {} checkpoints match (final state hash {:016x})",
                rep.cycles, rep.checked, rep.final_hash
            );
            Ok(())
        }
        Err(m) => bail!("{m}"),
    }
}

/// `twtool desync-diff`: plays two replays side by side (or one twice, the second with another human as the local
/// player) and names the first cycle at which their state hashes differ, and the first piece and field that differ.
/// Exits 0 when there is no difference and 1 when there is.
pub fn desync_diff(a: &Path, b: Option<&Path>, local: Option<u8>, install: &Path, map: Option<&Path>) -> Result<()> {
    let ra = read_replay(a)?;
    let (rb, label_b) = match (b, local) {
        (Some(p), _) => (read_replay(p)?, p.display().to_string()),
        (None, Some(slot)) => {
            let mut r = ra.clone();
            r.header = ra.header.with_local(slot)?;
            (r, format!("{} with player {slot} local", a.display()))
        }
        (None, None) => {
            bail!("give a second replay, or --local SLOT to run the first one again with another local player")
        }
    };
    let mut inst = Install::open(install)?;
    let mut wa = start(&mut inst, &ra.header, map)?;
    let mut wb = start(&mut inst, &rb.header, map)?;
    let cycles = ra.cycles.min(rb.cycles);
    println!("comparing {} with {label_b} over {cycles} cycles", a.display());
    match first_desync(&mut wa, &ra, &mut wb, &rb) {
        None => {
            println!("no difference: the state hashes agree at every one of {cycles} cycles");
            Ok(())
        }
        Some(d) => {
            println!(
                "first difference at cycle {}: state hash {:016x} vs {:016x}",
                d.cycle, d.left, d.right
            );
            match d.difference {
                Some(diff) => println!("first differing field: {diff}"),
                None => {
                    println!("the pieces of state compare equal; the hashes differ in a part this tool does not split")
                }
            }
            std::process::exit(1);
        }
    }
}
