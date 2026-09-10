//! Bounded backend-neutral BGRA frame transport.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use thiserror::Error;

/// Hard bounds negotiated before a producer can publish frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRingLimits {
    pub maximum_width: u32,
    pub maximum_height: u32,
    pub maximum_frame_bytes: usize,
}

impl FrameRingLimits {
    /// Computes a tightly packed BGRA8 byte bound.
    ///
    /// # Errors
    ///
    /// Returns [`FrameRingError::DimensionsOverflow`] if the dimensions cannot
    /// be represented by the current process address space.
    pub fn bgra(maximum_width: u32, maximum_height: u32) -> Result<Self, FrameRingError> {
        let row = usize::try_from(maximum_width)
            .ok()
            .and_then(|width| width.checked_mul(4))
            .ok_or(FrameRingError::DimensionsOverflow)?;
        let maximum_frame_bytes = usize::try_from(maximum_height)
            .ok()
            .and_then(|height| row.checked_mul(height))
            .ok_or(FrameRingError::DimensionsOverflow)?;
        Ok(Self {
            maximum_width,
            maximum_height,
            maximum_frame_bytes,
        })
    }
}

/// One complete, untorn BGRA8 frame copied out of the ring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BgraFrame {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub stride_bytes: u32,
    pub pixels: Vec<u8>,
}

/// Cumulative bounded-transport health counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameRingStats {
    pub published: u64,
    pub consumed: u64,
    pub dropped: u64,
    pub queued: usize,
}

#[derive(Debug)]
struct FrameSlot {
    ready: bool,
    sequence: u64,
    width: u32,
    height: u32,
    stride_bytes: u32,
    byte_length: usize,
    storage: Vec<u8>,
}

#[derive(Debug)]
struct RingState {
    slots: Vec<FrameSlot>,
    write_cursor: usize,
    queued: usize,
    next_sequence: u64,
    stats: FrameRingStats,
}

/// Thread-safe ring with preallocated slots and latest-frame consumption.
#[derive(Debug)]
pub struct FrameRing {
    limits: FrameRingLimits,
    state: Mutex<RingState>,
}

impl FrameRing {
    /// Allocates exactly `capacity * maximum_frame_bytes` pixel storage.
    ///
    /// # Errors
    ///
    /// Rejects zero capacity/limits and allocation-size overflow.
    pub fn new(capacity: usize, limits: FrameRingLimits) -> Result<Self, FrameRingError> {
        if capacity == 0 {
            return Err(FrameRingError::ZeroCapacity);
        }
        if limits.maximum_width == 0
            || limits.maximum_height == 0
            || limits.maximum_frame_bytes == 0
        {
            return Err(FrameRingError::ZeroDimensions);
        }
        capacity
            .checked_mul(limits.maximum_frame_bytes)
            .ok_or(FrameRingError::DimensionsOverflow)?;
        let slots = (0..capacity)
            .map(|_| FrameSlot {
                ready: false,
                sequence: 0,
                width: 0,
                height: 0,
                stride_bytes: 0,
                byte_length: 0,
                storage: vec![0; limits.maximum_frame_bytes],
            })
            .collect();
        Ok(Self {
            limits,
            state: Mutex::new(RingState {
                slots,
                write_cursor: 0,
                queued: 0,
                next_sequence: 1,
                stats: FrameRingStats::default(),
            }),
        })
    }

    /// Publishes a complete BGRA8 frame. An overrun replaces the oldest ring slot.
    ///
    /// # Errors
    ///
    /// Rejects invalid dimensions, stride, byte length, and poisoned synchronization.
    pub fn publish(
        &self,
        width: u32,
        height: u32,
        stride_bytes: u32,
        pixels: &[u8],
    ) -> Result<u64, FrameRingError> {
        let expected = self.validate_frame(width, height, stride_bytes, pixels.len())?;
        let mut state = self.state.lock().map_err(|_| FrameRingError::Poisoned)?;
        let index = state.write_cursor;
        let was_ready = state.slots[index].ready;
        let sequence = state.next_sequence;
        state.next_sequence = state.next_sequence.saturating_add(1);
        {
            let slot = &mut state.slots[index];
            slot.storage[..expected].copy_from_slice(pixels);
            slot.byte_length = expected;
            slot.width = width;
            slot.height = height;
            slot.stride_bytes = stride_bytes;
            slot.sequence = sequence;
            // The mutex release publishes all metadata/pixels as one untorn frame.
            slot.ready = true;
        }
        if was_ready {
            state.stats.dropped = state.stats.dropped.saturating_add(1);
        } else {
            state.queued += 1;
        }
        state.write_cursor = (index + 1) % state.slots.len();
        state.stats.published = state.stats.published.saturating_add(1);
        state.stats.queued = state.queued;
        Ok(sequence)
    }

    /// Returns the newest frame and discards any older queued frames.
    ///
    /// # Errors
    ///
    /// Returns a poisoned synchronization error if a producer panicked in the lock.
    pub fn consume_latest(&self) -> Result<Option<BgraFrame>, FrameRingError> {
        let mut state = self.state.lock().map_err(|_| FrameRingError::Poisoned)?;
        let latest = state
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.ready)
            .max_by_key(|(_, slot)| slot.sequence)
            .map(|(index, _)| index);
        let Some(latest) = latest else {
            return Ok(None);
        };
        let slot = &state.slots[latest];
        let frame = BgraFrame {
            sequence: slot.sequence,
            width: slot.width,
            height: slot.height,
            stride_bytes: slot.stride_bytes,
            pixels: slot.storage[..slot.byte_length].to_vec(),
        };
        let discarded = state.queued.saturating_sub(1);
        for slot in &mut state.slots {
            slot.ready = false;
        }
        state.queued = 0;
        state.stats.consumed = state.stats.consumed.saturating_add(1);
        state.stats.dropped = state
            .stats
            .dropped
            .saturating_add(u64::try_from(discarded).unwrap_or(u64::MAX));
        state.stats.queued = 0;
        Ok(Some(frame))
    }

    /// Returns current counters without changing queue state.
    ///
    /// # Errors
    ///
    /// Returns a poisoned synchronization error.
    pub fn stats(&self) -> Result<FrameRingStats, FrameRingError> {
        self.state
            .lock()
            .map(|state| state.stats)
            .map_err(|_| FrameRingError::Poisoned)
    }

    fn validate_frame(
        &self,
        width: u32,
        height: u32,
        stride_bytes: u32,
        supplied: usize,
    ) -> Result<usize, FrameRingError> {
        if width == 0 || height == 0 {
            return Err(FrameRingError::ZeroDimensions);
        }
        if width > self.limits.maximum_width || height > self.limits.maximum_height {
            return Err(FrameRingError::DimensionsOutOfRange {
                width,
                height,
                maximum_width: self.limits.maximum_width,
                maximum_height: self.limits.maximum_height,
            });
        }
        let minimum_stride = width
            .checked_mul(4)
            .ok_or(FrameRingError::DimensionsOverflow)?;
        if stride_bytes < minimum_stride || !stride_bytes.is_multiple_of(4) {
            return Err(FrameRingError::InvalidStride {
                supplied: stride_bytes,
                minimum: minimum_stride,
            });
        }
        let expected = usize::try_from(stride_bytes)
            .ok()
            .and_then(|stride| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| stride.checked_mul(height))
            })
            .ok_or(FrameRingError::DimensionsOverflow)?;
        if expected > self.limits.maximum_frame_bytes {
            return Err(FrameRingError::FrameTooLarge {
                supplied: expected,
                maximum: self.limits.maximum_frame_bytes,
            });
        }
        if supplied != expected {
            return Err(FrameRingError::ByteLength { supplied, expected });
        }
        Ok(expected)
    }
}

/// Frame validation and bounded-ring synchronization errors.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FrameRingError {
    #[error("frame ring capacity must be non-zero")]
    ZeroCapacity,
    #[error("frame dimensions must be non-zero")]
    ZeroDimensions,
    #[error("frame dimensions or allocation size overflow")]
    DimensionsOverflow,
    #[error("frame {width}x{height} exceeds negotiated maximum {maximum_width}x{maximum_height}")]
    DimensionsOutOfRange {
        width: u32,
        height: u32,
        maximum_width: u32,
        maximum_height: u32,
    },
    #[error("BGRA stride {supplied} is invalid; minimum aligned stride is {minimum}")]
    InvalidStride { supplied: u32, minimum: u32 },
    #[error("frame byte length {supplied} does not match stride/height length {expected}")]
    ByteLength { supplied: usize, expected: usize },
    #[error("frame uses {supplied} bytes, exceeding negotiated maximum {maximum}")]
    FrameTooLarge { supplied: usize, maximum: usize },
    #[error("frame ring synchronization was poisoned")]
    Poisoned,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(capacity: usize) -> FrameRing {
        FrameRing::new(capacity, FrameRingLimits::bgra(4, 4).unwrap()).unwrap()
    }

    #[test]
    fn latest_frame_wins_and_overflow_is_counted() {
        let ring = ring(2);
        let pixels = vec![1; 16];
        assert_eq!(ring.publish(2, 2, 8, &pixels).unwrap(), 1);
        assert_eq!(ring.publish(2, 2, 8, &[2; 16]).unwrap(), 2);
        assert_eq!(ring.publish(2, 2, 8, &[3; 16]).unwrap(), 3);
        let latest = ring.consume_latest().unwrap().unwrap();
        assert_eq!(latest.sequence, 3);
        assert!(latest.pixels.iter().all(|byte| *byte == 3));
        let stats = ring.stats().unwrap();
        assert_eq!(stats.published, 3);
        assert_eq!(stats.consumed, 1);
        assert_eq!(stats.dropped, 2);
        assert_eq!(stats.queued, 0);
    }

    #[test]
    fn malformed_dimensions_stride_and_length_are_rejected() {
        let ring = ring(2);
        assert!(matches!(
            ring.publish(5, 1, 20, &[0; 20]),
            Err(FrameRingError::DimensionsOutOfRange { .. })
        ));
        assert!(matches!(
            ring.publish(2, 2, 4, &[0; 8]),
            Err(FrameRingError::InvalidStride { .. })
        ));
        assert!(matches!(
            ring.publish(2, 2, 8, &[0; 15]),
            Err(FrameRingError::ByteLength { .. })
        ));
        assert_eq!(ring.stats().unwrap(), FrameRingStats::default());
    }

    #[test]
    fn empty_ring_returns_none() {
        assert_eq!(ring(1).consume_latest().unwrap(), None);
    }
}
