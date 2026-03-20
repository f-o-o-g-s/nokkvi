/// Trait for items that display collage artwork (genres, playlists).
///
/// This trait is defined in the data crate so that ViewModel types
/// can implement it without depending on iced. The iced-specific
/// collage artwork loading logic lives in the GUI crate's
/// `services/collage_artwork.rs`.
pub trait CollageArtworkItem {
    /// Unique identifier for this item
    fn id(&self) -> &str;
    /// Album IDs used to compose the collage (up to 9)
    fn artwork_album_ids(&self) -> &[String];
}
