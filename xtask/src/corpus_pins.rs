//! **Every test-corpus pin lives here** (`docs/development.md` §11). Corpus files are never
//! committed: `cargo xtask corpus --all` fetches them into `corpus/<set>/` (gitignored) from the
//! sources below and checks every file against `xtask/corpus/<set>.sha256`. The CI `corpus` job keys
//! its cache on this file and the manifests, so changing a pin is what makes CI fetch again.
//!
//! - Third-party sets come from their upstreams, under their own terms, and are never copied into
//!   our own corpus repository.
//! - Our own sets (`photos`, `heif`, `metadata`) come from <https://github.com/pkkid/dalica-corpus> (CC0 and
//!   public-domain files only, each with its source) at the commit pinned in
//!   [`DALICA_CORPUS_COMMIT`]; that repository's `SOURCES.tsv` gives every file's source, author,
//!   licence, size, sha256 and the tool that derived it.
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
/// `metadata` set added, `photos` and `heif` unchanged). `None` would mark its sets "not published
/// yet".
pub const DALICA_CORPUS_COMMIT: Option<&str> = Some("6239cad23d043ace64f171cadf951dd698568d01");

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

/// Every set, in the order `cargo xtask corpus` lists and fetches them.
pub const SETS: &[&Set] = &[&PNGSUITE, &RAW, &PHOTOS, &HEIF, &METADATA];
