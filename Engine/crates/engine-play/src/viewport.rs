//! Physical pixel sizing shared by embedded and native game views.

/// Upper bound for either dimension of a transported game frame.
pub const MAX_GAME_DIMENSION: u32 = 4096;

/// Fits the fixed 16:9 game view inside physical pixels, bounded for transport.
/// Zero-sized/minimized views retain a minimal valid render target.
pub fn game_view_extent([width, height]: [u32; 2]) -> [u32; 2] {
    let width = u64::from(width.max(1));
    let height = u64::from(height.max(1));
    let (width, height) = if width * 9 > height * 16 {
        (height * 16 / 9, height)
    } else {
        (width, (width * 9 / 16).max(1))
    };
    let largest = width.max(height);
    let divisor = largest.max(u64::from(MAX_GAME_DIMENSION));
    [width, height].map(|value| {
        u32::try_from((value * u64::from(MAX_GAME_DIMENSION) / divisor).max(1))
            .unwrap_or(MAX_GAME_DIMENSION)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_view_tracks_physical_size_and_letterboxing() {
        assert_eq!(game_view_extent([1920, 1080]), [1920, 1080]);
        assert_eq!(game_view_extent([1080, 1920]), [1080, 607]);
        assert_eq!(game_view_extent([2560, 1080]), [1920, 1080]);
        assert_eq!(game_view_extent([3840, 2160]), [3840, 2160]);
        assert_eq!(game_view_extent([7680, 4320]), [4096, 2304]);
        assert_eq!(game_view_extent([0, 0]), [1, 1]);
        assert_eq!(game_view_extent([u32::MAX, u32::MAX]), [4096, 2303]);
    }
}
