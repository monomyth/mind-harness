//! Benchmarks for performance-critical hot paths in mind-harness.
//!
//! Measures three areas targeted for optimization:
//! 1. FFT planner creation vs reuse
//! 2. Spectrogram full rebuild vs incremental update  
//! 3. get_data cloning overhead

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use std::f64::consts::PI;

fn generate_sine_wave(n: usize, sample_rate: f64, freq: f64) -> Vec<f64> {
    (0..n)
        .map(|i| (2.0 * PI * freq * i as f64 / sample_rate).sin())
        .collect()
}

fn bench_fft_planner(c: &mut Criterion) {
    let mut group = c.benchmark_group("fft_planner");
    
    let sample_rate = 250.0;
    let samples = generate_sine_wave(512, sample_rate, 10.0);
    
    group.bench_function("compute_fft_magnitude_512", |b| {
        b.iter(|| {
            mind_harness::fft::compute_fft_magnitude(
                black_box(&samples),
                black_box(sample_rate),
                black_box(100.0),
            )
        })
    });
    
    let samples_1024 = generate_sine_wave(1024, sample_rate, 10.0);
    group.bench_function("compute_fft_magnitude_1024", |b| {
        b.iter(|| {
            mind_harness::fft::compute_fft_magnitude(
                black_box(&samples_1024),
                black_box(sample_rate),
                black_box(100.0),
            )
        })
    });
    
    group.bench_function("fft_single_sided_uv_512", |b| {
        b.iter(|| {
            mind_harness::fft::fft_single_sided_uv(
                black_box(&samples),
                black_box(sample_rate),
            )
        })
    });
    
    group.bench_function("fft_display_uv_512", |b| {
        b.iter(|| {
            mind_harness::fft::fft_display_uv(
                black_box(&samples),
                black_box(sample_rate),
                black_box(60.0),
            )
        })
    });
    
    group.bench_function("band_powers_psd_512", |b| {
        b.iter(|| {
            mind_harness::fft::band_powers_psd(
                black_box(&samples),
                black_box(sample_rate),
            )
        })
    });
    
    group.finish();
}

fn bench_spectrogram_update(c: &mut Criterion) {
    let mut group = c.benchmark_group("spectrogram");
    
    let sample_rate = 250.0;
    let window_sec = 5.0;
    let n_samples = (window_sec * sample_rate) as usize;
    
    let samples = generate_sine_wave(n_samples, sample_rate, 10.0);
    let nfft = 256;
    let hop = (sample_rate / 20.0) as usize;
    
    group.bench_function("spectrogram_starts", |b| {
        b.iter(|| {
            mind_harness::widgets::spectrogram::spectrogram_starts(
                black_box(n_samples),
                black_box(nfft),
                black_box(hop),
            )
        })
    });
    
    group.bench_function("full_spectrogram_fft_loop", |b| {
        b.iter(|| {
            let starts = mind_harness::widgets::spectrogram::spectrogram_starts(n_samples, nfft, hop);
            for start in starts {
                let slice = &samples[start..start + nfft];
                let _ = mind_harness::fft::compute_fft_magnitude(
                    black_box(slice),
                    black_box(sample_rate),
                    black_box(60.0),
                );
            }
        })
    });
    
    group.finish();
}

fn bench_data_clone(c: &mut Criterion) {
    let mut group = c.benchmark_group("data_clone");
    
    for &n_samples in &[256, 512, 1024, 2048] {
        let data: Vec<Vec<f64>> = (0..n_samples)
            .map(|i| vec![i as f64; 12])
            .collect();
        
        group.bench_with_input(
            BenchmarkId::new("vec_clone", n_samples),
            &data,
            |b, data| {
                b.iter(|| {
                    let cloned: Vec<Vec<f64>> = black_box(data).clone();
                    cloned
                })
            },
        );
        
        group.bench_with_input(
            BenchmarkId::new("tail_slice_clone", n_samples),
            &data,
            |b, data| {
                let max_samples = n_samples / 2;
                b.iter(|| {
                    let start = data.len().saturating_sub(max_samples);
                    let cloned: Vec<Vec<f64>> = black_box(&data[start..]).to_vec();
                    cloned
                })
            },
        );
    }
    
    group.finish();
}

fn bench_hot_loop_simulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("hot_loop");
    
    let sample_rate = 250.0;
    let n_samples = 1250;
    let n_channels = 8;
    
    let data: Vec<Vec<f64>> = (0..n_samples)
        .map(|i| {
            (0..12)
                .map(|ch| {
                    if ch < n_channels {
                        (2.0 * PI * 10.0 * i as f64 / sample_rate).sin() * 50.0
                    } else {
                        0.0
                    }
                })
                .collect()
        })
        .collect();
    
    group.bench_function("simulated_fft_widget_update", |b| {
        b.iter(|| {
            let max_samples = 1024;
            let start = data.len().saturating_sub(max_samples);
            let fetched = &data[start..];
            
            for ch in 0..n_channels {
                let ch_data: Vec<f64> = fetched.iter().map(|row| row[ch]).collect();
                let _ = mind_harness::fft::fft_display_uv(
                    black_box(&ch_data),
                    black_box(sample_rate),
                    black_box(100.0),
                );
            }
        })
    });
    
    group.bench_function("simulated_band_power_update", |b| {
        b.iter(|| {
            let max_samples = 1024;
            let start = data.len().saturating_sub(max_samples);
            let fetched = &data[start..];
            
            for ch in 0..n_channels {
                let ch_data: Vec<f64> = fetched.iter().map(|row| row[ch]).collect();
                let _ = mind_harness::fft::band_powers_psd(
                    black_box(&ch_data),
                    black_box(sample_rate),
                );
            }
        })
    });
    
    group.finish();
}

criterion_group!(
    benches,
    bench_fft_planner,
    bench_spectrogram_update,
    bench_data_clone,
    bench_hot_loop_simulation,
);
criterion_main!(benches);
