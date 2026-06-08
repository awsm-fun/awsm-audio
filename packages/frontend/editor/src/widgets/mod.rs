//! Reusable UI atoms shared across the editor chrome: the line-icon set, the
//! button family, the segmented toggle, and the product brand mark. All resolve
//! their colors through the [`crate::theme`] design tokens.

// These atoms are a small reusable widget library; some builder methods exist
// for API completeness ahead of their first caller.
#![allow(dead_code)]

pub mod brand;
pub mod button;
pub mod icon;

pub use brand::brand;
pub use button::{Btn, BtnSize, BtnVariant, IconBtn};
pub use icon::Icon;
