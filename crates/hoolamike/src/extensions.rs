use {
    anyhow::Context,
    std::{
        fs::File,
        io::{Seek, SeekFrom, Write},
        path::Path,
    },
};

pub mod fallout_new_vegas_4gb_patch;
pub mod tale_of_two_wastelands_installer;
