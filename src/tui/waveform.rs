use std::collections::VecDeque;

/// Rolling waveform history for sparkline rendering.
/// Stores normalized amplitude levels 0..100.
pub struct WaveformHistory {
    data: VecDeque<u64>,
    capacity: usize,
    /// Smoothing for decay when idle.
    decay: f32,
}

#[allow(dead_code)]
impl WaveformHistory {
    pub fn new(capacity: usize) -> Self {
        Self {
            data: VecDeque::with_capacity(capacity),
            capacity,
            decay: 0.85,
        }
    }

    /// Push a pre-normalized level 0..100.
    pub fn push_level(&mut self, level: u64) {
        if self.data.len() >= self.capacity {
            self.data.pop_front();
        }
        self.data.push_back(level.min(100));
    }

    /// Compute a visual level from a raw audio chunk (f32 mono, 16kHz).
    /// Uses a mix of RMS and peak for punchy but stable bars.
    pub fn push_chunk(&mut self, chunk: &[f32]) {
        if chunk.is_empty() {
            self.push_level(0);
            return;
        }
        let level = chunk_to_level(chunk);
        self.push_level(level);
    }

    /// Push silence/decay step (call when no audio chunk arrived).
    pub fn push_silence(&mut self) {
        if let Some(&last) = self.data.back() {
            let decayed = (last as f32 * self.decay) as u64;
            if decayed < 2 {
                self.push_level(0);
            } else {
                self.push_level(decayed);
            }
        } else {
            self.push_level(0);
        }
    }

    pub fn data(&self) -> Vec<u64> {
        self.data.iter().copied().collect()
    }

    pub fn clear(&mut self) {
        self.data.clear();
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn max_level(&self) -> u64 {
        self.data.iter().copied().max().unwrap_or(0)
    }

    pub fn set_capacity(&mut self, cap: usize) {
        self.capacity = cap;
        while self.data.len() > cap {
            self.data.pop_front();
        }
    }
}

/// Convert a chunk of f32 samples (-1.0..1.0) into a 0..100 level.
pub fn chunk_to_level(chunk: &[f32]) -> u64 {
    if chunk.is_empty() {
        return 0;
    }
    let mut sum_sq = 0f32;
    let mut peak: f32 = 0.0;
    for &s in chunk {
        let v = s.abs();
        if v > peak {
            peak = v;
        }
        sum_sq += s * s;
    }
    let rms = (sum_sq / chunk.len() as f32).sqrt();
    // RMS is more stable, peak gives attack. Blend heavily toward peak for visual punch
    // but keep RMS to avoid flicker on transients.
    // Empirically: speech rms 0.02..0.08, peak 0.2..0.6
    let rms_part = (rms * 420.0).clamp(0.0, 70.0);
    let peak_part = (peak * 95.0).clamp(0.0, 100.0);
    let level = rms_part * 0.45 + peak_part * 0.55;
    // Small gate: below ~1% treat as silence to keep line flat
    if level < 2.5 {
        0
    } else {
        level.clamp(0.0, 100.0) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_zero() {
        assert_eq!(chunk_to_level(&[0.0; 128]), 0);
        assert_eq!(chunk_to_level(&[0.0001; 128]), 0);
    }

    #[test]
    fn loud_signal_high() {
        let chunk = vec![0.5; 512];
        assert!(chunk_to_level(&chunk) > 40);
    }

    #[test]
    fn history_capacity() {
        let mut w = WaveformHistory::new(4);
        for i in 0..6 {
            w.push_level(i);
        }
        assert_eq!(w.len(), 4);
        assert_eq!(w.data(), vec![2, 3, 4, 5]);
    }

    #[test]
    fn decay_reduces() {
        let mut w = WaveformHistory::new(10);
        w.push_level(80);
        w.push_silence();
        assert!(w.data().last().unwrap() < &80);
    }
}
