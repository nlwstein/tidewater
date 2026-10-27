//! Readers for the data formats of a user-supplied *Warcraft II: Battle.net Edition* installation.
//!
//! Every format detail is cited in the module that uses it. Nothing here is derived from the original program's code.

pub mod aibin;
pub mod alow;
pub mod fnt;
pub mod grp;
pub mod identity;
pub mod install;
pub mod mods;
pub mod mpq;
mod mpq_codec;
pub mod pcx;
pub mod pud;
pub mod raw;
pub mod script;
pub mod smk;
pub mod sprites;
pub mod tbl;
pub mod testmap;
pub mod testmpq;
pub mod tileset;
pub mod udta;
pub mod ugrd;
