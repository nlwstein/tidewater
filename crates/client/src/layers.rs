//! The screen's layers as pictures the core owns (`docs/design/client-core.md`, migration step 3): each layer is a
//! picture in [`Pictures`] and a [`Layer`] that says where it goes on the screen. They replace the sprites and image
//! assets the layers used to be; nothing here depends on an engine.
//!
//! A layer is on one of three planes, composed in this order (`shell`):
//!
//! | Plane | Placed |
//! |---|---|
//! | [`Plane::Map`] | In the map view: its centre in map pixels (y down), drawn at the view's zoom |
//! | [`Plane::Interface`] | Centred on the screen, one picture pixel per screen pixel |
//! | [`Plane::Flow`] | Its centre this far from the screen's centre (y down), at its size, with its grey tint |
//!
//! Within a plane the layers go back to front by `z`, and in the order they were made where `z` is equal.
//!
//! The layers themselves are in a [`Stack`] (migration step 9): each with its [`Role`], which says what finds it
//! again (the map view's picture, the game menu, a flow screen's background), the flow screen that owns it (it goes
//! when that screen is left) and its [`Fade`]. A layer's picture goes with it.

use std::collections::HashMap;

use crate::scene::Picture;
use crate::states::AppState;

/// A picture in [`Pictures`]. Ids are never used again, so a stale one finds nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PictureId(u64);

/// The layers' pictures.
#[derive(Default)]
pub struct Pictures {
    next: u64,
    pictures: HashMap<PictureId, Stored>,
    revisions: u64,
}

/// A part of a picture: x, y, width and height in pixels.
pub type Part = (u32, u32, u32, u32);

/// Each row of `part` in a picture `width` pixels wide, as the indices of its pixels (row by row from the top left).
pub fn rows(part: Part, width: u32) -> impl Iterator<Item = std::ops::Range<usize>> {
    let (x, y, w, h) = part;
    (y..y + h).map(move |row| {
        let at = row as usize * width as usize + x as usize;
        at..at + w as usize
    })
}

/// [`rows`] as the bytes of an RGBA8 picture.
pub fn rgba_rows(part: Part, width: u32) -> impl Iterator<Item = std::ops::Range<usize>> {
    rows(part, width).map(|r| r.start * 4..r.end * 4)
}

struct Stored {
    picture: Picture,
    /// A number no other change has had, taken whenever the picture may change.
    revision: u64,
    /// When only parts of it changed: the revision before, and the parts.
    changed: Option<(u64, Vec<Part>)>,
}

impl Stored {
    fn new(picture: Picture, revision: u64) -> Self {
        Stored {
            picture,
            revision,
            changed: None,
        }
    }
}

impl PictureId {
    /// The id as a number, for the screen's draw list (`compose::Source`).
    pub fn raw(self) -> u64 {
        self.0
    }

    pub fn from_raw(id: u64) -> Self {
        PictureId(id)
    }
}

impl Pictures {
    fn revision(&mut self) -> u64 {
        self.revisions += 1;
        self.revisions
    }

    pub fn add(&mut self, p: Picture) -> PictureId {
        let id = PictureId(self.next);
        self.next += 1;
        let r = self.revision();
        self.pictures.insert(id, Stored::new(p, r));
        id
    }

    pub fn get(&self, id: PictureId) -> Option<&Picture> {
        self.pictures.get(&id).map(|s| &s.picture)
    }

    /// The picture and its revision.
    pub fn get_with_revision(&self, id: PictureId) -> Option<(&Picture, u64)> {
        self.pictures.get(&id).map(|s| (&s.picture, s.revision))
    }

    /// The picture, to change: it takes a new revision.
    pub fn get_mut(&mut self, id: PictureId) -> Option<&mut Picture> {
        let r = self.revision();
        self.pictures.get_mut(&id).map(|s| {
            s.revision = r;
            s.changed = None;
            &mut s.picture
        })
    }

    /// The picture, to change only `parts` of (its size stays): it takes a new revision, and [`Pictures::changed`]
    /// says which parts changed since the one before, so a copy of it elsewhere (the GPU's texture) can take only
    /// those.
    pub fn get_mut_parts(&mut self, id: PictureId, parts: Vec<Part>) -> Option<&mut Picture> {
        let r = self.revision();
        self.pictures.get_mut(&id).map(|s| {
            s.changed = Some((s.revision, parts));
            s.revision = r;
            &mut s.picture
        })
    }

    /// The revision before the last change and the parts it changed, if it changed only parts.
    pub fn changed(&self, id: PictureId) -> Option<(u64, &[Part])> {
        self.pictures
            .get(&id)
            .and_then(|s| s.changed.as_ref())
            .map(|(r, parts)| (*r, parts.as_slice()))
    }

    /// Replaces picture `id`, if it is still there.
    pub fn set(&mut self, id: PictureId, p: Picture) {
        let r = self.revision();
        if let Some(old) = self.pictures.get_mut(&id) {
            *old = Stored::new(p, r);
        }
    }

    pub fn remove(&mut self, id: PictureId) -> Option<Picture> {
        self.pictures.remove(&id).map(|s| s.picture)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.pictures.len()
    }
}

/// Which part of the screen a layer belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plane {
    Map,
    Interface,
    Flow,
}

/// One layer of the screen: a picture and its place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer {
    pub picture: PictureId,
    pub plane: Plane,
    /// Back to front within the plane.
    pub z: f32,
    /// The picture's centre: in map pixels on the map plane, from the screen's centre on the flow plane (y down on
    /// both); unused on the interface plane.
    pub at: (f32, f32),
    /// The size it is drawn at, in map or screen pixels; `None`: the picture's own size.
    pub size: Option<(f32, f32)>,
    /// A grey tint (sRGB, 0 to 1): 1 leaves the colours as they are.
    pub tint: f32,
    pub visible: bool,
}

impl Layer {
    pub fn new(picture: PictureId, plane: Plane, z: f32) -> Layer {
        Layer {
            picture,
            plane,
            z,
            at: (0.0, 0.0),
            size: None,
            tint: 1.0,
            visible: true,
        }
    }

    pub fn at(self, x: f32, y: f32) -> Layer {
        Layer { at: (x, y), ..self }
    }

    #[cfg(test)]
    pub fn sized(self, w: f32, h: f32) -> Layer {
        Layer {
            size: Some((w, h)),
            ..self
        }
    }

    /// The size it is drawn at, for a picture `pic`.
    pub fn drawn_size(&self, pic: &Picture) -> (f32, f32) {
        self.size.unwrap_or((pic.width as f32, pic.height as f32))
    }
}

/// What a layer is, for the code that finds it again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// A flow screen's layer (`frontend`): 0 the background, 1 what is drawn over it. The flow's fades act on these.
    Flow(u8),
    /// The game menu's dialog (`gamemenu`).
    Menu,
    /// The game screen's layers: the interface picture, the terrain, the map view's picture and the placement image.
    Hud,
    Terrain,
    View,
    Ghost,
}

impl Role {
    /// The game screen's own layers, which go when a game ends; the flow's outlive it.
    pub fn of_game(self) -> bool {
        matches!(self, Role::Hud | Role::Terrain | Role::View | Role::Ghost)
    }
}

/// A brightness ramp on a layer: its tint goes from `from` to `to` over `ms`, starting at `at` (the flow screen's
/// clock, in ms).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fade {
    pub from: f32,
    pub to: f32,
    pub at: f64,
    pub ms: f64,
}

impl Fade {
    /// The tint at `clock`.
    pub fn tint(&self, clock: f64) -> f32 {
        let k = ((clock - self.at) / self.ms.max(1.0)).clamp(0.0, 1.0) as f32;
        self.from + (self.to - self.from) * k
    }
}

/// A layer on the screen, with what it is and who owns it.
#[derive(Clone, Debug)]
pub struct Entry {
    pub layer: Layer,
    pub role: Role,
    /// The flow screen it belongs to: it goes when the screen is left (`states`, a change's step 3).
    pub owner: Option<AppState>,
    pub fade: Option<Fade>,
}

/// The screen's layers, in the order they were made.
#[derive(Default)]
pub struct Stack {
    entries: Vec<Entry>,
}

impl Stack {
    pub fn add(&mut self, layer: Layer, role: Role, owner: Option<AppState>) {
        self.entries.push(Entry {
            layer,
            role,
            owner,
            fade: None,
        });
    }

    /// The layers, in the order they were made.
    pub fn iter(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Entry> {
        self.entries.iter_mut()
    }

    /// The first layer with `role`.
    pub fn get_mut(&mut self, role: Role) -> Option<&mut Layer> {
        self.entries.iter_mut().find(|e| e.role == role).map(|e| &mut e.layer)
    }

    pub fn has(&self, role: Role) -> bool {
        self.entries.iter().any(|e| e.role == role)
    }

    /// Removes the layers `gone` picks, and their pictures.
    pub fn remove(&mut self, pictures: &mut Pictures, gone: impl Fn(&Entry) -> bool) {
        self.entries.retain(|e| {
            let go = gone(e);
            if go {
                pictures.remove(e.layer.picture);
            }
            !go
        });
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_removed_layer_takes_its_picture() {
        let mut pictures = Pictures::default();
        let mut stack = Stack::default();
        let a = pictures.add(pic(1, 1));
        let b = pictures.add(pic(1, 1));
        stack.add(Layer::new(a, Plane::Flow, 0.0), Role::Flow(0), Some(AppState::Title));
        stack.add(Layer::new(b, Plane::Map, 0.0), Role::View, None);
        stack.remove(&mut pictures, |e| e.role.of_game());
        assert_eq!(stack.len(), 1);
        assert!(pictures.get(b).is_none() && pictures.get(a).is_some());
        assert!(stack.has(Role::Flow(0)) && !stack.has(Role::View));
    }

    #[test]
    fn a_fade_runs_from_its_start_over_its_length() {
        let f = Fade {
            from: 0.0,
            to: 1.0,
            at: 100.0,
            ms: 200.0,
        };
        assert_eq!(f.tint(50.0), 0.0);
        assert_eq!(f.tint(200.0), 0.5);
        assert_eq!(f.tint(400.0), 1.0);
    }

    fn pic(w: u32, h: u32) -> Picture {
        Picture {
            width: w,
            height: h,
            rgba: vec![0; (w * h * 4) as usize],
        }
    }

    #[test]
    fn ids_are_never_used_again() {
        let mut p = Pictures::default();
        let a = p.add(pic(1, 1));
        assert!(p.remove(a).is_some());
        let b = p.add(pic(2, 2));
        assert_ne!(a, b);
        assert!(p.get(a).is_none());
        p.set(a, pic(3, 3));
        assert!(p.get(a).is_none(), "a removed picture stays removed");
        assert_eq!(p.len(), 1);
    }

    #[test]
    fn a_change_of_parts_says_which_parts_since_which_revision() {
        let mut p = Pictures::default();
        let a = p.add(pic(4, 4));
        let (_, before) = p.get_with_revision(a).expect("added");
        assert_eq!(p.changed(a), None, "a new picture is new all over");
        p.get_mut_parts(a, vec![(1, 1, 2, 2)]).expect("there");
        let (_, after) = p.get_with_revision(a).expect("there");
        assert_ne!(before, after);
        assert_eq!(p.changed(a), Some((before, &[(1, 1, 2, 2)][..])));
        p.get_mut(a).expect("there");
        assert_eq!(p.changed(a), None, "a change of the whole");
        p.get_mut_parts(a, vec![(0, 0, 1, 1)]).expect("there");
        p.set(a, pic(2, 2));
        assert_eq!(p.changed(a), None, "a new picture");
    }

    #[test]
    fn a_layer_is_drawn_at_its_picture_size_unless_sized() {
        let p = pic(4, 3);
        let id = Pictures::default().add(pic(4, 3));
        let l = Layer::new(id, Plane::Flow, 1.0);
        assert_eq!(l.drawn_size(&p), (4.0, 3.0));
        assert_eq!(l.sized(8.0, 6.0).drawn_size(&p), (8.0, 6.0));
        assert_eq!(l.at(2.0, -1.0).at, (2.0, -1.0));
    }
}
