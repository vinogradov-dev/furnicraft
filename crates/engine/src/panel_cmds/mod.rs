//! Document commands for a panel, in the three areas its data divides into.
//!
//! `panel.rs` holds the panel itself — creation, stored outline, name, nominal size, material, end
//! bevel;
//! `machining.rs` its machining records; `banding.rs` the edge and corner trims of its outline.
//! This module holds no command: it declares the areas and re-exports every id, label, parameter
//! type and spec, so the registry and the UI keep reaching a command through `panel_cmds`.
//!
//! Changes when: a panel command area is added, split or removed.

mod banding;
mod machining;
mod panel;

pub(crate) use banding::{
    BOW_CLEAR, BOW_SET, CORNER_BANDING_CLEAR, CORNER_BANDING_SET, CORNER_CLEAR, CORNER_SET, EDGE_BANDING_CLEAR,
    EDGE_BANDING_SET, RING_BANDING_CLEAR, RING_BANDING_SET,
};
pub use banding::{
    BOW_CLEAR_ID, BOW_CLEAR_LABEL, BOW_SET_ID, BOW_SET_LABEL, CORNER_BANDING_CLEAR_ID, CORNER_BANDING_CLEAR_LABEL,
    CORNER_BANDING_SET_ID, CORNER_BANDING_SET_LABEL, CORNER_CLEAR_ID, CORNER_CLEAR_LABEL, CORNER_SET_ID,
    CORNER_SET_LABEL, EDGE_BANDING_CLEAR_ID, EDGE_BANDING_CLEAR_LABEL, EDGE_BANDING_SET_ID, EDGE_BANDING_SET_LABEL,
    RING_BANDING_CLEAR_ID, RING_BANDING_CLEAR_LABEL, RING_BANDING_SET_ID, RING_BANDING_SET_LABEL,
};
pub use banding::{
    PanelBowClear, PanelBowSet, PanelCornerBandingClear, PanelCornerBandingSet, PanelCornerClear, PanelCornerSet,
    PanelEdgeBandingClear, PanelEdgeBandingSet, PanelRingBandingClear, PanelRingBandingSet,
};

pub(crate) use machining::{MACHINING_ADD, MACHINING_MOVE, MACHINING_REMOVE, MACHINING_UPDATE};
pub use machining::{
    MACHINING_ADD_ID, MACHINING_ADD_LABEL, MACHINING_MOVE_ID, MACHINING_MOVE_LABEL, MACHINING_REMOVE_ID,
    MACHINING_REMOVE_LABEL, MACHINING_UPDATE_ID, MACHINING_UPDATE_LABEL,
};
pub use machining::{PanelMachiningAdd, PanelMachiningMove, PanelMachiningRemove, PanelMachiningUpdate};

pub(crate) use panel::{
    BEVEL_CLEAR, BEVEL_SET, CONTOUR_CREATE, CREATE, MATERIAL_SET, OUTLINE_CONTOUR, OUTLINE_SET, RENAME, RESIZE,
};
pub use panel::{
    BEVEL_CLEAR_ID, BEVEL_CLEAR_LABEL, BEVEL_SET_ID, BEVEL_SET_LABEL, CONTOUR_CREATE_ID, CONTOUR_CREATE_LABEL,
    CREATE_ID, CREATE_LABEL, MATERIAL_SET_ID, MATERIAL_SET_LABEL, OUTLINE_CONTOUR_ID, OUTLINE_CONTOUR_LABEL,
    OUTLINE_SET_ID, OUTLINE_SET_LABEL, RENAME_ID, RENAME_LABEL, RESIZE_ID, RESIZE_LABEL,
};
pub use panel::{
    PanelBevelClear, PanelBevelSet, PanelContourCreate, PanelCreate, PanelMaterialSet, PanelOutlineSet,
    PanelOutlineToContour, PanelRename, PanelResize,
};
