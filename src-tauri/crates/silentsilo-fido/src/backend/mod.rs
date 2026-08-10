#[cfg(all(feature = "hardware", not(windows)))]
mod ctap;

#[cfg(all(feature = "hardware", windows))]
mod win;

#[cfg(all(feature = "hardware", not(windows)))]
pub(crate) use ctap::*;

#[cfg(all(feature = "hardware", windows))]
pub(crate) use win::*;

#[cfg(not(feature = "hardware"))]
mod stub;

#[cfg(not(feature = "hardware"))]
pub(crate) use stub::*;
