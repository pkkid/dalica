//! **Every test-corpus pin lives here** (`docs/development.md` §11). Corpus files are never
//! committed: `cargo xtask corpus --all` fetches them into `corpus/<set>/` (gitignored) from the
//! sources below and checks every file against `xtask/corpus/<set>.sha256`. The CI `corpus` job keys
//! its cache on this file and the manifests, so changing a pin is what makes CI fetch again.
//!
//! - Third-party sets come from their upstreams, under their own terms, and are never copied into
//!   our own corpus repository.
//! - Our own sets (`photos`, `heif`, `metadata`, `public-faces`) come from <https://github.com/pkkid/dalica-corpus> (CC0 and
//!   public-domain files only, each with its source) at the commit pinned in
//!   [`DALICA_CORPUS_COMMIT`]; the `video` set's phone clips (CC BY-SA 4.0, CC0 and public domain)
//!   are assets of that repository's `video-v1` release, each pinned by URL and sha256. That
//!   repository's `SOURCES.tsv` gives every file's source, author, licence, size, sha256 and the
//!   tool that derived it. `heif-wide` is made locally by
//!   `cargo xtask corpus make heif-wide` and stays "not published yet" until the maintainer
//!   decides to publish it there.
//!
//! - The face models (`face-models`) are third-party files fetched from their own repositories at
//!   the addresses `dalica-faces` pins in `crates/faces/src/known.rs` (reserved decision 5), never
//!   copied into our corpus repository or any release.
//!
//! Moving a pin: change it here, run `cargo xtask corpus --update-manifest <set>`, review the
//! manifest diff, run `cargo xtask test-corpus` and raise the floors that improved (never lower one).

/// Where a set's files come from.
pub enum Source {
    /// One archive at a fixed URL, checked against `sha256` before anything in it is unpacked.
    /// Every file in it lands in `corpus/<set>/` (the manifest says which and what they hold).
    Archive { url: &'static str, sha256: &'static str },
    /// Single files, each downloaded from its own URL and checked against its own sha256.
    Files(&'static [PinnedFile]),
    /// A directory of our own corpus repository at a pinned commit, fetched as GitHub's archive of
    /// that commit; `None` until the set is published and pinned.
    Repo { repo: &'static str, commit: Option<&'static str>, subdir: &'static str },
}

/// One file of a [`Source::Files`] set.
pub struct PinnedFile {
    /// The file's name in `corpus/<set>/`.
    pub name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    /// What it is (camera and mode), for `cargo xtask corpus` and `SOURCES.md`.
    pub what: &'static str,
}

/// A test corpus: `corpus/<name>/`, listed by `xtask/corpus/<name>.sha256`.
pub struct Set {
    pub name: &'static str,
    /// One line for `cargo xtask corpus`.
    pub what: &'static str,
    /// Where it comes from and under which terms (written into `corpus/<name>/SOURCES.md`).
    pub terms: &'static str,
    /// The crates whose corpus tests read it (`cargo xtask test-corpus -p <crate>`).
    pub used_by: &'static [&'static str],
    pub source: Source,
}

/// Our own CC0 corpus repository (`docs/development.md` §11).
pub const DALICA_CORPUS_REPO: &str = "pkkid/dalica-corpus";
/// The pinned commit of [`DALICA_CORPUS_REPO`] (M6.42: the `photos` and `heif` sets; M6.34: the
/// `metadata` set added; M13.17: the `public-faces` set added, the other sets unchanged). `None`
/// would mark its sets "not published yet".
pub const DALICA_CORPUS_COMMIT: Option<&str> = Some("f3dd25cc3111f397f80157adefe956ecb9489e6d");

/// PngSuite: Willem van Schaik's PNG test images, the 2017-07-19 release archive. Its own terms:
/// "Permission to use, copy, modify and distribute these images for any purpose and without fee
/// is hereby granted" (`PngSuite.LICENSE`, inside the archive). The site serves it over plain HTTP
/// only (its HTTPS endpoint fails the TLS handshake), so the archive's sha256 below is what makes
/// the download trustworthy.
pub const PNGSUITE: Set = Set {
    name: "pngsuite",
    what: "PngSuite (176 PNGs: every bit depth, colour type, interlacing, chunk, and 14 corrupt files)",
    terms: "PngSuite by Willem van Schaik, from http://www.schaik.com/pngsuite/ (release archive PngSuite-2017jul19.tgz), \
            unmodified. Its own permissive terms: permission to use, copy, modify and distribute these images for any purpose \
            and without fee is granted (see PngSuite.LICENSE).",
    used_by: &["dalica-image"],
    source: Source::Archive {
        url: "http://www.schaik.com/pngsuite/PngSuite-2017jul19.tgz",
        sha256: "0294b244c95a8342c01b00010cf34abdcabc7c6a34fd0fe1bd963917537bfdc8",
    },
};

/// The download URL of a raw.pixls.us sample by its id and published name.
macro_rules! pixls {
    ($id:literal, $nice:literal) => {
        concat!("https://raw.pixls.us/getfile.php/", $id, "/nice/", $nice)
    };
}

/// Camera raw samples from <https://raw.pixls.us>, two per container kind Dalica reads previews
/// from (M5.9, M5.10), the smallest CC0 files of a camera of each era. Every file below is marked
/// CC0 (Creative Commons Zero, public domain) on the site, checked on 2026-10-12, and its sha256 is
/// the one the site publishes.
pub const RAW_FILES: &[PinnedFile] = &[
    PinnedFile {
        name: "Canon_EOS_20D.CR2",
        url: pixls!("1018", "Canon%20-%20EOS%2020D%20-%20RAW%20%283:2%29.CR2"),
        sha256: "db78660d48589130d99e0bc621424851eb0f84df052634fb5bc89c7a02fe7033",
        what: "Canon EOS 20D, RAW (3:2), 7.1 MB",
    },
    PinnedFile {
        name: "Canon_EOS_40D.CR2",
        url: pixls!("2101", "Canon%20-%20EOS%2040D%20-%20RAW%20%283:2%29.CR2"),
        sha256: "775c806358fedddec7622113a5e399a3330bd5a65e35e37ae1108d8bf58067be",
        what: "Canon EOS 40D, RAW (3:2), 10.4 MB",
    },
    PinnedFile {
        name: "Canon_EOS_R6.CR3",
        url: pixls!("4659", "Canon%20-%20EOS%20R6%20-%203:2.CR3"),
        sha256: "74abb0a113d075ad9887a058082f40dd2a938c4813a08474d82356f11a027778",
        what: "Canon EOS R6 (3:2), 5.0 MB",
    },
    PinnedFile {
        name: "Canon_EOS_RP.CR3",
        url: pixls!("4671", "Canon%20-%20EOS%20RP%20-%203:2.CR3"),
        sha256: "4a859ab137f792aedaf3416b0aad1497ebaab02052a49453902e226699154aa7",
        what: "Canon EOS RP (3:2), 7.1 MB",
    },
    PinnedFile {
        name: "Canon_PowerShot_G1.CRW",
        url: pixls!("2073", "Canon%20-%20PowerShot%20G1%20-%20RAW%20%284:3%29.crw"),
        sha256: "f39be5acf14057ce04527fd8d0574e57ec8ed5fed082d7bfaff763075a44ee8a",
        what: "Canon PowerShot G1, RAW (4:3), 2.2 MB",
    },
    PinnedFile {
        name: "Canon_EOS_D30.CRW",
        url: pixls!("1307", "Canon%20-%20EOS%20D30%20-%20RAW%20%283:2%29.CRW"),
        sha256: "4ee482f2b6f9f1a8d370703ea04e9f2abaa49356973a7f56b229af93a78e8a2e",
        what: "Canon EOS D30, RAW (3:2), 2.9 MB",
    },
    PinnedFile {
        name: "Google_Pixel_6_Pro.DNG",
        url: pixls!("5961", "Google%20-%20Pixel%206%20Pro%20-%2016bit%20%284:3%29.dng"),
        sha256: "3a859e917a91698a3e15ce3b3721a48a1fb1296873e0697d3bf7c2793297200d",
        what: "Google Pixel 6 Pro phone DNG, 16-bit (4:3), 11.4 MB",
    },
    PinnedFile {
        name: "Pentax_K-x.DNG",
        url: pixls!("830", "Pentax%20-%20K-x%20-%2012bit%20%283:2%29.DNG"),
        sha256: "7a594b66b54f99bb5aba4627c78abfc41213d6911ea54e4f5d2dd6c8505f6968",
        what: "Pentax K-x in-camera DNG, 12-bit (3:2), 13.7 MB",
    },
    PinnedFile {
        name: "Nikon_D2H.NEF",
        url: pixls!("5227", "Nikon%20-%20D2H%20-%2012bit%2012bit%20compressed%20%28Lossy%20%28type%201%29%29%20%283:2%29.NEF"),
        sha256: "155edb938f884ea7372ce98d4ff5f965c3e413b43b95bc9923da6e92082cf914",
        what: "Nikon D2H, 12-bit lossy compressed (3:2), 3.1 MB",
    },
    PinnedFile {
        name: "Nikon_Z6_3.NEF",
        url: pixls!("7810", "Nikon%20-%20Z6_3%20-%208bit%20compressed%20%283:2%29.NEF"),
        sha256: "a8fa2cacadcf9dc85475ebcf37fcc32614072bc155e39d01491cb9e293c1343d",
        what: "Nikon Z6 III, compressed (3:2), 6.2 MB",
    },
    PinnedFile {
        name: "Sony_ILCE-7S.ARW",
        url: pixls!("1582", "Sony%20-%20ILCE-7S%20-%2014bit%2014bit%20compressed%20%283:2%29.ARW"),
        sha256: "a35ebb2fbec929daa5beb20d1ce5c15a8aac7b1a7a231455387f3df8a7442e07",
        what: "Sony ILCE-7S, 14-bit compressed (3:2), 5.9 MB",
    },
    PinnedFile {
        name: "Sony_DSLR-A100.ARW",
        url: pixls!("2346", "Sony%20-%20DSLR-A100%20-%20compressed%20%283:2%29.ARW"),
        sha256: "4df7981ac4baec9648b4f106751ea4b09a6e27bc70a15f8ef09bec051fec362d",
        what: "Sony DSLR-A100, compressed (3:2), 9.7 MB",
    },
    PinnedFile {
        name: "Pentax_K10D.PEF",
        url: pixls!("2239", "Pentax%20-%20K10D%20-%2012bit%2012bit%20compressed%20%283:2%29.PEF"),
        sha256: "e35ae4154a468be3154f5f462e884ba5941f010d3e8f23d347fbec14809f44d3",
        what: "Pentax K10D, 12-bit compressed (3:2), 9.1 MB",
    },
    PinnedFile {
        name: "Pentax_istDL.PEF",
        url: pixls!("1083", "Pentax%20-%20%2Aist%20DL%20-%2012bit%20%283:2%29.PEF"),
        sha256: "0e6cd3c999a6dbf121591b2c58aaa8216e625f549db46af627f7346ccf1cf662",
        what: "Pentax *ist DL, 12-bit (3:2), 9.9 MB",
    },
    PinnedFile {
        name: "Olympus_C5060WZ.ORF",
        url: pixls!("1799", "Olympus%20-%20C5060WZ%20-%2012bit%20%284:3%29.ORF"),
        sha256: "3fc12bc3f6320cfab456111d7805b446e329fdd1907680be55e2e163e5e084bd",
        what: "Olympus C5060WZ, 12-bit (4:3), 7.3 MB",
    },
    PinnedFile {
        name: "Olympus_E-3.ORF",
        url: pixls!("1803", "Olympus%20-%20E-3%20-%2016bit%20%284:3%29.ORF"),
        sha256: "ba4605bbd9fe9b77971da444bcdb1698904c0ad4ebdf9dd42dd45ab255f3f6d4",
        what: "Olympus E-3 (4:3), 9.2 MB",
    },
    PinnedFile {
        name: "Panasonic_DMC-LX7.RW2",
        url: pixls!("7010", "Panasonic%20-%20DMC-LX7%20-%203:2.RW2"),
        sha256: "18dfc3877e944020299635a8c2fad61fca2485b1c9b67645a495ee870a86e599",
        what: "Panasonic DMC-LX7 (3:2), 3.2 MB",
    },
    PinnedFile {
        name: "Panasonic_DMC-FZ28.RW2",
        url: pixls!("7330", "Panasonic%20-%20DMC-FZ28%20-%2016:9.RW2"),
        sha256: "b58564eb3cd148e6282aeba0b104506fa021ce905a3040f15b6ef1cbd6142fff",
        what: "Panasonic DMC-FZ28 (16:9), 8.8 MB",
    },
    PinnedFile {
        name: "Samsung_NX500.SRW",
        url: pixls!("3668", "Samsung%20-%20NX500%20-%2012bit%2012bit%20normal%20compression%20%283:2%29.SRW"),
        sha256: "156e43118811bcecb6fcc600b5f41ee1773c4f00b6a5b2dd72e177e35f6596ab",
        what: "Samsung NX500, 12-bit compressed (3:2), 19.8 MB",
    },
    PinnedFile {
        name: "Samsung_NX_mini.SRW",
        url: pixls!("3765", "Samsung%20-%20NX%20mini%20-%2012bit%20%283:2%29.SRW"),
        sha256: "8e1bd19fff8c3b458f63bd6cf0ceda97e708efaf576424cf6c8ebf58eb0c3fc3",
        what: "Samsung NX mini, 12-bit (3:2), 20.1 MB",
    },
    PinnedFile {
        name: "Fujifilm_FinePix_S5000.RAF",
        url: pixls!("2726", "Fujifilm%20-%20FinePix%20S5000%20-%204:3.RAF"),
        sha256: "dabd5e74521a6980156be9fd4b88d0c37b0fe4d0e0e6f5c12db8cffff1b76297",
        what: "Fujifilm FinePix S5000 (4:3), 6.5 MB",
    },
    PinnedFile {
        name: "Fujifilm_X-T30_II.RAF",
        url: pixls!("5064", "Fujifilm%20-%20X-T30%20II%20-%2014bit%2014bit%20compressed%20%283:2%29.RAF"),
        sha256: "5e2678038377cc7f146ceb7693c08535fa1af162cec705d06b0446c3d3017d47",
        what: "Fujifilm X-T30 II, 14-bit compressed (3:2), 18.5 MB",
    },
    PinnedFile {
        name: "Minolta_DiMAGE_5.MRW",
        url: pixls!("7795", "Minolta%20-%20DiMAGE%205%20-%204:3.MRW"),
        sha256: "d60bfd80bcb1f7b9c88b14a5e46ea27885bbf92d990ab30b3939288678fea0d9",
        what: "Minolta DiMAGE 5 (4:3), 6.1 MB",
    },
    PinnedFile {
        name: "Minolta_Dynax_7D.MRW",
        url: pixls!("1826", "Minolta%20-%20Dynax%207D%20-%203:2.MRW"),
        sha256: "713819e33e5302ec864d576d57223cfc8dcf46e8d45e0ae744d1fec40290a89f",
        what: "Minolta Dynax 7D (3:2), 8.8 MB",
    },
    PinnedFile {
        name: "Sigma_DP1.X3F",
        url: pixls!("1116", "Sigma%20-%20DP1%20-%203:2.X3F"),
        sha256: "44508f7df4191aacaecb6461e1841dae002e5b71a4f95a00aa3f52d1c6d6bc5f",
        what: "Sigma DP1 (3:2), 9.8 MB",
    },
    PinnedFile {
        name: "Sigma_SD14.X3F",
        url: pixls!("4462", "Sigma%20-%20SIGMA%20SD14%20-%203:2.X3F"),
        sha256: "c32f247eb372c3e66ad3b433518ec11f898fdc317c8432ebe025b46d4917bcf3",
        what: "Sigma SD14 (3:2), 11.6 MB",
    },
];

/// `corpus/raw/`: [`RAW_FILES`].
pub const RAW: Set = Set {
    name: "raw",
    what: "camera raws from raw.pixls.us, two per kind: CR2, CR3, CRW, DNG, NEF, ARW, PEF, ORF, RW2, SRW, RAF, MRW, X3F",
    terms: "Camera raw samples from https://raw.pixls.us, each released by its contributor under CC0 1.0 (public domain) \
            as the site states for every file pinned here; unmodified.",
    used_by: &["dalica-image"],
    source: Source::Files(RAW_FILES),
};

/// `corpus/photos/`: CC0 camera and phone JPEGs from our own corpus repository.
pub const PHOTOS: Set = Set {
    name: "photos",
    what: "57 CC0/public-domain photos from 18 camera and phone makes: JPEG orientations 0-8 and none, EXIF thumbnails, MPF, \
           Display P3 and Adobe RGB, CMYK JPEG and TIFF, progressive, truncated, 16-bit TIFF and PNG, WebP",
    terms: "`photos/` of https://github.com/pkkid/dalica-corpus: CC0 and public-domain files only, each with its source in that \
            repository.",
    used_by: &["dalica-image"],
    source: Source::Repo { repo: DALICA_CORPUS_REPO, commit: DALICA_CORPUS_COMMIT, subdir: "photos" },
};

/// `corpus/heif/`: small HEIC/HEIF files from our own corpus repository (the `heif` feature).
pub const HEIF: Set = Set {
    name: "heif",
    what: "3 CC0/public-domain iPhone HEIC photos (grid tiles, EXIF, colour) for the heif feature",
    terms: "`heif/` of https://github.com/pkkid/dalica-corpus: CC0 and public-domain files only, each with its source in that \
            repository.",
    used_by: &["dalica-image"],
    source: Source::Repo { repo: DALICA_CORPUS_REPO, commit: DALICA_CORPUS_COMMIT, subdir: "heif" },
};

/// `corpus/heif-wide/`: HEIF files made from `dalica-scenes` pictures by libheif's `heif-enc`
/// (M10.24), every layout the `heif` feature meets: made locally by `cargo xtask corpus make
/// heif-wide` and measured by `dalica-image`'s `tests/heif_wide.rs`. Not published until the
/// maintainer decides (the M10.R question); publishing it pins it like the other sets.
pub const HEIF_WIDE: Set = Set {
    name: "heif-wide",
    what: "36 HEIF files made by libheif's heif-enc from dalica-scenes pictures: 8/10/12 bits; 4:2:0, 4:2:2, 4:4:4 and \
           monochrome; alpha; grids; irot and imir; ICC and nclx; a thumbnail; a sequence, mini, unci, JPEG-coded and AV1 files",
    terms: "Generated from Dalica's own procedural scenes (dalica-scenes), CC0 1.0; each file's sources and heif-enc arguments \
            are in its recipes.tsv.",
    used_by: &["dalica-image"],
    source: Source::Repo { repo: DALICA_CORPUS_REPO, commit: None, subdir: "heif-wide" },
};

/// `corpus/metadata/`: `dalica-scenes` photos after other applications wrote metadata into them
/// (M6.34), from our own corpus repository; each file's tool and version is in its `SOURCES.tsv`.
pub const METADATA: Set = Set {
    name: "metadata",
    what: "53 generated files: dalica-scenes JPEG, PNG, WebP and TIFF photos after exiv2, ImageMagick and Pillow wrote captions, \
           keywords, ratings, GPS, dates, MWG face regions and foreign metadata into them; a DNG-like TIFF and RAW .xmp sidecars",
    terms: "`metadata/` of https://github.com/pkkid/dalica-corpus: generated from Dalica's own procedural scenes, CC0 1.0, each \
            file's base scene, tool and version in that repository's SOURCES.tsv.",
    used_by: &["dalica-meta"],
    source: Source::Repo { repo: DALICA_CORPUS_REPO, commit: DALICA_CORPUS_COMMIT, subdir: "metadata" },
};

/// `corpus/public-faces/`: public-domain portraits, several of each person, one folder per person
/// (M13.17), from our own corpus repository; every file's source page, author and licence template
/// is in its `SOURCES.tsv`. Also holds the reference vectors the corpus test checks embeddings
/// against (`reference/<model id>.tsv`, written by `dalica-faces`' own pipeline).
pub const PUBLIC_FACES: Set = Set {
    name: "public-faces",
    what: "45 public-domain portraits of 10 people (3 to 6 each; official portraits across decades and candid shots with other \
           faces), US federal government works from Wikimedia Commons, and the reference vectors of the built-in recognisers",
    terms: "`public-faces/` of https://github.com/pkkid/dalica-corpus: US federal government works (public domain, each file's \
            PD-USGov template on its Commons page), Commons' 1280-pixel renditions, each with its source in that repository's \
            SOURCES.tsv.",
    used_by: &["dalica-faces"],
    source: Source::Repo { repo: DALICA_CORPUS_REPO, commit: DALICA_CORPUS_COMMIT, subdir: "public-faces" },
};

/// The built-in face models (reserved decision 5, `faces.md` §3.2), each at the address
/// `crates/faces/src/known.rs` pins to a commit of the model's own repository, with the SHA-256
/// it records there (the `dalica-faces` corpus test checks both lists agree).
pub const FACE_MODEL_FILES: &[PinnedFile] = &[
    PinnedFile {
        name: "face_detection_yunet_2023mar.onnx",
        url: "https://github.com/opencv/opencv_zoo/raw/25f423d0e04c31a17254620e58febd7386da523b/models/face_detection_yunet/face_detection_yunet_2023mar.onnx",
        sha256: "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4",
        what: "YuNet 2023mar face detector (OpenCV Zoo), MIT, 0.2 MB",
    },
    PinnedFile {
        name: "face_recognition_sface_2021dec.onnx",
        url: "https://github.com/opencv/opencv_zoo/raw/25f423d0e04c31a17254620e58febd7386da523b/models/face_recognition_sface/face_recognition_sface_2021dec.onnx",
        sha256: "0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79",
        what: "SFace 2021dec face recogniser (OpenCV Zoo), labelled Apache-2.0, 38.7 MB",
    },
    PinnedFile {
        name: "glintr100.onnx",
        url: "https://huggingface.co/fal/AuraFace-v1/resolve/af6d057c9b0ec4071d4c49c80e3539258798b609/glintr100.onnx",
        sha256: "a7933ea5330113b01c9b60351d8f4c33003f145d8470ac5f0e52ee2effe25c60",
        what: "AuraFace v1 face recogniser (fal.ai), Apache-2.0, 260.7 MB",
    },
];

/// `corpus/face-models/`: [`FACE_MODEL_FILES`].
pub const FACE_MODELS: Set = Set {
    name: "face-models",
    what: "the built-in face models: the YuNet 2023mar detector, the SFace 2021dec and AuraFace v1 recognisers",
    terms: "Third-party model weights fetched from their own repositories, unmodified, for the dalica-faces corpus test only; \
            never copied into dalica-corpus, this repository or a release. YuNet: MIT (OpenCV Zoo). SFace: labelled Apache-2.0 \
            by OpenCV Zoo, training data undocumented. AuraFace v1: Apache-2.0 (fal.ai), training data undisclosed.",
    used_by: &["dalica-faces"],
    source: Source::Files(FACE_MODEL_FILES),
};

/// The download URL of a phone clip: an asset of our corpus repository's `video-v1` release.
macro_rules! corpus_video {
    ($name:literal) => {
        concat!("https://github.com/pkkid/dalica-corpus/releases/download/video-v1/", $name)
    };
}

/// Phone clips for the video decoders (`docs/video-decoders.md` §9, M15.46's record; M15.15): H.264
/// and HEVC (Main, Main 10 HLG with Dolby Vision) from iPhones and Android phones, with AAC sound.
/// Each is an unmodified download from its Internet Archive item, under the licence its uploader
/// set, published to our corpus repository as an asset of its `video-v1` release (the maintainer's
/// answer at M15.R, 2026-10-20; that repository's `SOURCES.tsv` and `video/README.md` give each
/// clip's item, author and licence). Release assets, not files at a pinned commit: three are over
/// GitHub's 100 MB file limit, and the other sets are fetched as commit archives. Read by
/// `crates/image/tests/video_frames.rs`.
pub const VIDEO_FILES: &[PinnedFile] = &[
    PinnedFile {
        name: "IMG_0444.MOV",
        url: corpus_video!("IMG_0444.MOV"),
        sha256: "42c1f6d143e25f00e88ad8281a54c2e1d325e27afc6d73bb6eaaf16238d32a53",
        what: "iPhone 7 Plus, HEVC Main 1080p30, 51 frames (CC BY-SA 4.0, ankkumar), 2.0 MB",
    },
    PinnedFile {
        name: "IMG_0445.MOV",
        url: corpus_video!("IMG_0445.MOV"),
        sha256: "c4da19d500667740dafe4201d5a3245dbaa38a2cbb1793d345be46e6f2f2f3ef",
        what: "iPhone 7 Plus, HEVC Main 1080p30, AAC-LC mono (CC BY-SA 4.0, ankkumar), 24.4 MB",
    },
    PinnedFile {
        name: "IMG_0477.MOV",
        url: corpus_video!("IMG_0477.MOV"),
        sha256: "1f1a7a969c1831e1d7b5fd545fb341d64cbb54854d8dc16b2742b82f9716eb86",
        what: "iPhone 7 Plus, HEVC Main 1080p30, portrait (CC BY-SA 4.0, ankkumar), 21.3 MB",
    },
    PinnedFile {
        name: "IMG_0488.MOV",
        url: corpus_video!("IMG_0488.MOV"),
        sha256: "241f8f6af0a7e9ef7b233efe2c492256db863eb46000453c5ceb2948b9d816db",
        what: "iPhone 7 Plus, HEVC Main 1080p30, portrait (CC BY-SA 4.0, ankkumar), 36.7 MB",
    },
    PinnedFile {
        name: "IMG_6851.MOV",
        url: corpus_video!("IMG_6851.MOV"),
        sha256: "811d06bf464975c5131b8bcf6258d8cbc033b4c94e8986a0f636251df34842bb",
        what: "iPhone 13, H.264 High 1080p29.97, portrait, AAC-LC stereo (public domain mark), 14.3 MB",
    },
    PinnedFile {
        name: "20260418_0635.MOV",
        url: corpus_video!("20260418_0635.MOV"),
        sha256: "e465cf1d5c9572d06f302a555356f6b4e53313f1fa3e319405edb04af13aa79c",
        what: "iPhone 7 Plus, H.264 High 1080p30, portrait (CC0), 156.6 MB",
    },
    PinnedFile {
        name: "BTS_iPhone12mini_4K60.MOV",
        url: corpus_video!("BTS_iPhone12mini_4K60.MOV"),
        sha256: "66e4127d06bd75bd707c2b7e566438eb27990e1049364effde80c2a8d9c81332",
        what: "iPhone 12 mini, H.264 High 4K 59.94, portrait (public domain mark), 94.3 MB",
    },
    PinnedFile {
        name: "20260404_201346.mp4",
        url: corpus_video!("20260404_201346.mp4"),
        sha256: "30316225ce021c1bb9a620700d24cfdc800871aec8ebc0031aa0e5191b4b096d",
        what: "Android 14, H.264 High 1080p30, portrait, AAC-LC 48 kHz stereo (CC0), 62.8 MB",
    },
    PinnedFile {
        name: "VID_20261017_160853309.mp4",
        url: corpus_video!("VID_20261017_160853309.mp4"),
        sha256: "34bde67731965ad0a085a63f7327d13af8b8a0ff322011fb0419c75358af715a",
        what: "moto g play (2023), H.264 High 1080p29.6, landscape (CC BY-SA 4.0, outerpasta), 41.1 MB",
    },
    PinnedFile {
        name: "VID_20261017_141936777.mp4",
        url: corpus_video!("VID_20261017_141936777.mp4"),
        sha256: "7fc67fb2a16ccb59276f4ef90853050a2db5ff932f4ae22e7726fd8d00e744f3",
        what: "moto g play (2023), H.264 High 1080p29.6, portrait (CC BY-SA 4.0, outerpasta), 17.0 MB",
    },
    PinnedFile {
        name: "M-6_Fun_Fact.MOV",
        url: corpus_video!("M-6_Fun_Fact.MOV"),
        sha256: "6508907b38cc63e9a9d6af8868ec61a271a17a57962e622bbd561b975d43e326",
        what: "iPhone 13, HEVC Main 10 HLG, Dolby Vision 8.4, 1080p60, portrait (CC0), 119.0 MB",
    },
    PinnedFile {
        name: "104-107_Fun_Fact.MOV",
        url: corpus_video!("104-107_Fun_Fact.MOV"),
        sha256: "474d46d61bd7d7192cd8176b6ad39a281ce2661066f1349a2b74b9dbe001fb48",
        what: "iPhone 13, HEVC Main 10 HLG, Dolby Vision 8.4, 1080p60 (CC0), 187.9 MB",
    },
];

/// The phone clips of [`VIDEO_FILES`].
pub const VIDEO: Set = Set {
    name: "video",
    what: "12 phone clips (H.264 and HEVC, 1080p to 4K, HDR, with AAC sound), about 740 MB",
    terms: "Phone clips from the Internet Archive (https://archive.org/details/<item>), unmodified, each under the licence its \
            uploader set: CC BY-SA 4.0 (Ank Kumar's Zurich E-Prix 2018 clips; Alex Barry's ROG LVL 20 videos), CC0, or the \
            public domain mark; downloaded from the video-v1 release of https://github.com/pkkid/dalica-corpus, whose \
            SOURCES.tsv and video/README.md give each file's item, author and licence.",
    used_by: &["dalica-image"],
    source: Source::Files(VIDEO_FILES),
};

/// Every set, in the order `cargo xtask corpus` lists and fetches them.
pub const SETS: &[&Set] = &[&PNGSUITE, &RAW, &PHOTOS, &HEIF, &HEIF_WIDE, &METADATA, &PUBLIC_FACES, &FACE_MODELS, &VIDEO];
