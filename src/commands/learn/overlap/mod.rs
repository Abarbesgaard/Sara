mod files;
mod hints;
mod tags;
mod warn;

pub(super) use tags::TagOverlap;
pub(super) use warn::warn_overlap;

#[cfg(test)]
pub(super) use {
    files::file_overlaps,
    hints::{
        canonical_derived_count, canonical_hint, near_dupe_suggestion, partial_overlap_suggestion,
    },
    warn::check_overlap,
};
