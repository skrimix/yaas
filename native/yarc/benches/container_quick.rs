use std::{
    hint::black_box,
    io::Cursor,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use criterion::{Criterion, SamplingMode, Throughput, criterion_group, criterion_main};
use tempfile::tempdir;
use tokio::runtime::Runtime;
use yarc::container::{CompressionScheme, PayloadKind, YarcReader, YarcWriter};

const BENCH_KEY: [u8; 32] = [7; 32];
const CHUNK_LOG2: u8 = 22;
const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;
const BYTE_PAYLOAD_SIZE: usize = 24 * MIB;

#[derive(Clone, Copy)]
struct FixtureFileSpec {
    path: &'static str,
    size_bytes: usize,
}

#[derive(Clone, Copy)]
struct ReleaseFixtureSpec {
    name: &'static str,
    files: &'static [FixtureFileSpec],
}

const SOURCE_SHAPED_RELEASES: &[ReleaseFixtureSpec] = &[
    ReleaseFixtureSpec {
        name: "Project Atlas v120+2.4.1 -A01",
        files: &[
            FixtureFileSpec {
                path: "pkg.sample.atlas/main.120.pkg.sample.atlas.obb",
                size_bytes: 10 * MIB,
            },
            FixtureFileSpec {
                path: "pkg.sample.atlas.apk",
                size_bytes: MIB,
            },
        ],
    },
    ReleaseFixtureSpec {
        name: "Project Beacon v241+3.1.0 -B02",
        files: &[
            FixtureFileSpec {
                path: "pkg.sample.beacon/main.241.pkg.sample.beacon.obb",
                size_bytes: 9 * MIB,
            },
            FixtureFileSpec {
                path: "pkg.sample.beacon.apk",
                size_bytes: MIB,
            },
        ],
    },
    ReleaseFixtureSpec {
        name: "Project Cinder v77+1.0.77 -C03",
        files: &[
            FixtureFileSpec {
                path: "pkg.sample.cinder/main.77.pkg.sample.cinder.obb",
                size_bytes: 7 * MIB,
            },
            FixtureFileSpec {
                path: "pkg.sample.cinder.apk",
                size_bytes: MIB,
            },
            FixtureFileSpec {
                path: "EMPTY.txt",
                size_bytes: 0,
            },
        ],
    },
    ReleaseFixtureSpec {
        name: "Project Drift v58+0.9.58 -D04",
        files: &[
            FixtureFileSpec {
                path: "pkg.sample.drift/main.58.pkg.sample.drift.obb",
                size_bytes: 7 * MIB,
            },
            FixtureFileSpec {
                path: "pkg.sample.drift.apk",
                size_bytes: MIB,
            },
        ],
    },
    ReleaseFixtureSpec {
        name: "Project Ember v410+4.1.0 -E05",
        files: &[FixtureFileSpec {
            path: "pkg.sample.ember.apk",
            size_bytes: 4 * MIB,
        }],
    },
    ReleaseFixtureSpec {
        name: "Project Flux v389+3.8.9 -F06",
        files: &[FixtureFileSpec {
            path: "pkg.sample.flux.apk",
            size_bytes: 3 * MIB,
        }],
    },
    ReleaseFixtureSpec {
        name: "Project Grove v22+1.2.2 -G07",
        files: &[FixtureFileSpec {
            path: "pkg.sample.grove.apk",
            size_bytes: 2 * MIB,
        }],
    },
    ReleaseFixtureSpec {
        name: "Utility Pack v510+5.1.0",
        files: &[
            FixtureFileSpec {
                path: "addons/pkg.tools.bridge.apk",
                size_bytes: 2 * MIB,
            },
            FixtureFileSpec {
                path: "addons/pkg.tools.installer.apk",
                size_bytes: 512 * KIB,
            },
            FixtureFileSpec {
                path: "pkg.tools.shell.apk",
                size_bytes: MIB,
            },
            FixtureFileSpec {
                path: "README.txt",
                size_bytes: 32 * KIB,
            },
        ],
    },
    ReleaseFixtureSpec {
        name: "Launcher Pack v320+3.2.0 -OSS",
        files: &[
            FixtureFileSpec {
                path: "StarterLauncher-3.2.0.apk",
                size_bytes: MIB,
            },
            FixtureFileSpec {
                path: "NOTES.txt",
                size_bytes: 32 * KIB,
            },
        ],
    },
    ReleaseFixtureSpec {
        name: "Patch Bundle v3+3 -P09",
        files: &[
            FixtureFileSpec {
                path: "apply_patch.sh",
                size_bytes: 128 * KIB,
            },
            FixtureFileSpec {
                path: "README.txt",
                size_bytes: 32 * KIB,
            },
            FixtureFileSpec {
                path: "payload.apk",
                size_bytes: 128 * KIB,
            },
        ],
    },
    ReleaseFixtureSpec {
        name: "Patch Bundle Notes",
        files: &[
            FixtureFileSpec {
                path: "README.txt",
                size_bytes: 32 * KIB,
            },
            FixtureFileSpec {
                path: "payload.apk",
                size_bytes: 64 * KIB,
            },
        ],
    },
];

#[derive(Clone, Copy)]
enum FixtureKind {
    Compressible,
    Random,
}

impl FixtureKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Compressible => "compressible",
            Self::Random => "random",
        }
    }
}

fn fixture_kinds() -> [FixtureKind; 2] {
    [FixtureKind::Compressible, FixtureKind::Random]
}

fn quick_config() -> Criterion {
    Criterion::default()
        .sample_size(30)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
}

fn fixture_bytes(kind: FixtureKind, size: usize, seed: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(size);
    let mut block_index = 0_u64;

    while out.len() < size {
        let block = match kind {
            FixtureKind::Compressible => blake3::hash(&seed.to_le_bytes()),
            FixtureKind::Random => {
                let mut input = [0_u8; 16];
                input[..8].copy_from_slice(&seed.to_le_bytes());
                input[8..].copy_from_slice(&block_index.to_le_bytes());
                blake3::hash(&input)
            }
        };
        out.extend_from_slice(block.as_bytes());
        block_index += 1;
    }

    out.truncate(size);
    out
}

fn compression_configs() -> [CompressionScheme; 2] {
    [CompressionScheme::None, CompressionScheme::Zstd]
}

fn runtime() -> Runtime {
    Runtime::new().expect("tokio runtime should initialize")
}

async fn make_fixture_tree(root: &Path, kind: FixtureKind) {
    let mut seed = 0_u64;

    for release in SOURCE_SHAPED_RELEASES {
        for file in release.files {
            let path = root.join(release.name).join(file.path);
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .expect("fixture tree should be created");
            }

            let payload = fixture_bytes(kind, file.size_bytes, seed);
            tokio::fs::write(&path, &payload)
                .await
                .expect("fixture file should be written");
            seed += 1;
        }
    }
}

fn fixture_tree_size_bytes() -> u64 {
    SOURCE_SHAPED_RELEASES
        .iter()
        .flat_map(|release| release.files.iter())
        .map(|file| file.size_bytes as u64)
        .sum()
}

fn bench_encrypt_decrypt_bytes(c: &mut Criterion) {
    let runtime = runtime();
    let mut group = c.benchmark_group("container_bytes_quick");
    group.sampling_mode(SamplingMode::Flat);

    for fixture_kind in fixture_kinds() {
        let payload = fixture_bytes(fixture_kind, BYTE_PAYLOAD_SIZE, 0);
        let writer = YarcWriter::new(BENCH_KEY, CHUNK_LOG2);
        let reader = YarcReader::new(BENCH_KEY);
        group.throughput(Throughput::Bytes(payload.len() as u64));

        for compression in compression_configs() {
            let encrypt_label = format!(
                "encrypt/{}/{}/chunk_{CHUNK_LOG2}",
                fixture_kind.as_str(),
                compression.as_str()
            );
            group.bench_function(&encrypt_label, |b| {
                b.to_async(&runtime).iter(|| async {
                    let finished = writer
                        .encrypt_bytes_with_compression(
                            PayloadKind::Manifest,
                            black_box(&payload),
                            Vec::new(),
                            compression,
                        )
                        .await
                        .expect("byte encryption should succeed");
                    black_box(finished.summary.container_len);
                });
            });

            let container: Arc<[u8]> = Arc::from(
                runtime
                    .block_on(writer.encrypt_bytes_with_compression(
                        PayloadKind::Manifest,
                        &payload,
                        Vec::new(),
                        compression,
                    ))
                    .expect("fixture container should be created")
                    .out,
            );

            let decrypt_label = format!(
                "decrypt/{}/{}/chunk_{CHUNK_LOG2}",
                fixture_kind.as_str(),
                compression.as_str()
            );
            group.bench_function(&decrypt_label, |b| {
                let container = Arc::clone(&container);
                b.to_async(&runtime).iter(|| async {
                    let (bytes, header) = reader
                        .decrypt_to_bytes(
                            Cursor::new(black_box(Arc::clone(&container))),
                            PayloadKind::Manifest,
                        )
                        .await
                        .expect("byte decryption should succeed");
                    black_box(bytes.len());
                    black_box(header.version());
                });
            });
        }
    }

    group.finish();
}

fn bench_archive_extract_dir(c: &mut Criterion) {
    let runtime = runtime();
    let mut group = c.benchmark_group("container_directory_quick");
    group.sampling_mode(SamplingMode::Flat);

    for fixture_kind in fixture_kinds() {
        let source = tempdir().expect("fixture tempdir should be created");
        runtime.block_on(make_fixture_tree(source.path(), fixture_kind));

        let writer = YarcWriter::new(BENCH_KEY, CHUNK_LOG2);
        let reader = YarcReader::new(BENCH_KEY);
        group.throughput(Throughput::Bytes(fixture_tree_size_bytes()));

        for compression in compression_configs() {
            let archive_label = format!(
                "archive/{}/{}/chunk_{CHUNK_LOG2}",
                fixture_kind.as_str(),
                compression.as_str()
            );
            group.bench_function(&archive_label, |b| {
                b.to_async(&runtime).iter(|| async {
                    let finished = writer
                        .archive_directory_with_compression(
                            black_box(source.path()),
                            Vec::new(),
                            compression,
                        )
                        .await
                        .expect("directory archive should succeed");
                    black_box(finished.summary.container_len);
                });
            });

            let container: Arc<[u8]> = Arc::from(
                runtime
                    .block_on(writer.archive_directory_with_compression(
                        source.path(),
                        Vec::new(),
                        compression,
                    ))
                    .expect("fixture directory container should be created")
                    .out,
            );

            let extract_label = format!(
                "extract/{}/{}/chunk_{CHUNK_LOG2}",
                fixture_kind.as_str(),
                compression.as_str()
            );
            group.bench_function(&extract_label, |b| {
                let container = Arc::clone(&container);
                let reader = reader.clone();
                b.to_async(&runtime).iter_custom(|iters| {
                    let container = Arc::clone(&container);
                    let reader = reader.clone();
                    async move {
                        let mut elapsed = Duration::ZERO;

                        for _ in 0..iters {
                            let target = tempdir().expect("target tempdir should be created");
                            let started = Instant::now();
                            let header = reader
                                .extract_to_directory(
                                    Cursor::new(black_box(Arc::clone(&container))),
                                    target.path(),
                                )
                                .await
                                .expect("directory extract should succeed");
                            elapsed += started.elapsed();
                            black_box(header.version());
                            drop(target);
                        }

                        elapsed
                    }
                });
            });
        }
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = quick_config();
    targets = bench_encrypt_decrypt_bytes, bench_archive_extract_dir
}
criterion_main!(benches);
