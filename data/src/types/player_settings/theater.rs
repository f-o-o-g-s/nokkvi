//! Theater Mode settings.

use serde::{Deserialize, Serialize};

use crate::define_labeled_enum;

define_labeled_enum! {
    /// How Theater Mode shows the player bar.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum TheaterControls {
        /// Slides away after a few idle seconds; any activity brings it back.
        #[default]
        AutoHide { label: "Auto-hide", wire: "auto_hide" },
        /// Always on screen.
        AlwaysShown { label: "Always shown", wire: "always_shown" },
        /// Never on screen; hotkeys still work.
        AlwaysHidden { label: "Always hidden", wire: "always_hidden" },
    }
}
