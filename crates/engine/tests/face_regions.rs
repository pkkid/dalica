//! People names in photo files (`roadmap.md` M13.19; `xmp-interop.md` §6, `data-model.md` §5) on
//! temporary JPEGs holding MWG regions written by another application: their faces and people
//! come into the library on the first scan (a name matched to a person ignoring case, else
//! created; no undo step, nothing written back) and a face added to a file later comes in on the
//! rescan; with Store people names in photos on, naming a face writes a region that reads back,
//! renaming the person rewrites it and undo writes the old value; with the option off nothing is
//! written, and Write People Names to Photos… (`faces.writeXmp`) writes after its question; a face
//! another application added is brought in before a write, never written over; the file's pet
//! region survives every write; the owner thread makes no file system call.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::common::{jpeg, off_owner, open_with, read};
use dalica_core::{PersonId, PhotoId, Rect01, SystemClock};
use dalica_engine::cmd::face_regions::question;
use dalica_engine::{CmdError, Session};
use dalica_fs::Expect;
use dalica_library::{Face, FacePerson};
use dalica_meta::Limits;
use dalica_meta::fields::FaceRegion;
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(30);

const MWG: &str = r#"xmlns:mwg-rs="http://www.metadataworkinggroup.com/schemas/regions/" xmlns:stArea="http://ns.adobe.com/xmp/sType/Area#""#;

/// The pet region another application wrote: it must survive every write of Dalica's.
const PET: &str = r#"mwg-rs:Name="Rex" mwg-rs:Type="Pet""#;

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect01 {
    Rect01::new(x0, y0, x1, y1).unwrap()
}

/// One region entry: `name` (none for an unnamed face), `kind` (`Face`, `Pet`) and its box.
fn region(name: Option<&str>, kind: &str, r: Rect01) -> String {
    let name = name.map_or(String::new(), |n| format!(r#"mwg-rs:Name="{n}" "#));
    let (x, y, w, h) = ((r.x0() + r.x1()) / 2.0, (r.y0() + r.y1()) / 2.0, r.width(), r.height());
    format!(
        r#"      <rdf:li rdf:parseType="Resource" {name}mwg-rs:Type="{kind}"><mwg-rs:Area stArea:x="{x}" stArea:y="{y}" stArea:w="{w}" stArea:h="{h}" stArea:unit="normalized"/></rdf:li>"#
    )
}

/// A JPEG of scene `seed` holding an XMP packet with `regions` (and a foreign property), as
/// another application writes it.
fn with_regions(seed: u64, regions: &[String]) -> Vec<u8> {
    let packet = format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  <rdf:Description rdf:about=\"\" xmlns:zz=\"http://example.com/zz/\" {MWG}>\n   <zz:keep>other application</zz:keep>\n   <mwg-rs:Regions rdf:parseType=\"Resource\"><mwg-rs:RegionList><rdf:Bag>\n{}\n   </rdf:Bag></mwg-rs:RegionList></mwg-rs:Regions>\n  </rdf:Description>\n</rdf:RDF></x:xmpmeta>\n{}<?xpacket end=\"w\"?>",
        regions.join("\n"),
        " ".repeat(256)
    );
    let plain = jpeg(seed);
    let mut payload = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    payload.extend_from_slice(packet.as_bytes());
    let mut out = plain[..2].to_vec();
    out.extend([0xff, 0xe1]);
    out.extend(u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
    out.extend(payload);
    out.extend(&plain[2..]);
    out
}

const ANA: [f64; 4] = [0.2, 0.25, 0.4, 0.55];
const BRUNO: [f64; 4] = [0.5, 0.2, 0.7, 0.5];
const NOBODY: [f64; 4] = [0.75, 0.6, 0.95, 0.9];
const REX: [f64; 4] = [0.05, 0.6, 0.3, 0.95];
const CARLA: [f64; 4] = [0.05, 0.05, 0.15, 0.2];

fn r(b: [f64; 4]) -> Rect01 {
    rect(b[0], b[1], b[2], b[3])
}

/// Whether two boxes agree to 10⁻⁵ (an area's centre and size round on the way through a file).
fn close(a: &Rect01, b: [f64; 4]) -> bool {
    [a.x0() - b[0], a.y0() - b[1], a.x1() - b[2], a.y1() - b[3]].iter().all(|d| d.abs() < 1e-5)
}

/// A library watching one folder `Lisbon` holding `files` (and `sidecar`, when given); `before`
/// runs on the session before the folder is added.
struct Lib {
    _library: tempfile::TempDir,
    _disk: tempfile::TempDir,
    folder: PathBuf,
    s: Session,
}

impl Lib {
    fn new(files: &[(&str, Vec<u8>)], sidecar: Option<Value>, before: impl FnOnce(&mut Session)) -> Lib {
        let (library, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let folder = disk.path().join("Lisbon");
        dalica_fs::create_dir_all(&folder).unwrap();
        for (name, bytes) in files {
            dalica_fs::replace(&folder.join(name), bytes, Expect::Absent).unwrap();
        }
        if let Some(v) = sidecar {
            dalica_fs::replace(&folder.join(".dalica.json"), serde_json::to_vec(&v).unwrap().as_slice(), Expect::Absent).unwrap();
        }
        let mut s = open_with(library.path(), Arc::new(SystemClock));
        before(&mut s);
        s.execute("folders.add", &json!({ "path": folder })).unwrap();
        s.wait_idle();
        assert_eq!(s.library().photos().count(), files.len(), "every file is a photo: {:?}", s.scan_warnings());
        Lib { _library: library, _disk: disk, folder, s }
    }

    fn id(&self, name: &str) -> PhotoId {
        self.s.library().photos().find(|p| p.name == name).unwrap().id
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e:?}"))
    }

    fn faces(&self, name: &str) -> Vec<Face> {
        self.s.library().faces(self.id(name)).to_vec()
    }

    fn person(&self, name: &str) -> PersonId {
        self.s.library().person_by_name(name).cloned().unwrap_or_else(|| panic!("a person called {name}"))
    }

    fn name_of(&self, face: &Face) -> Option<String> {
        face.person.person().and_then(|p| self.s.library().person(p)).map(|p| p.name.clone())
    }

    /// Writes what is queued, applies what comes back, and writes again what that queued (a write
    /// made again after an import); no write fails.
    fn flush(&mut self) {
        for _ in 0..3 {
            assert!(self.s.flush_metadata(WAIT), "the writes landed in time");
            self.s.poll();
        }
        assert_eq!(self.s.pending_metadata_writes(), 0);
        assert_eq!(self.s.metadata_events().count(), 0, "{:?}", self.s.metadata_events().collect::<Vec<_>>());
    }

    fn path(&self, name: &str) -> PathBuf {
        self.folder.join(name)
    }

    /// The face regions of the file, read off the owner thread.
    fn file_faces(&self, name: &str) -> Vec<FaceRegion> {
        dalica_meta::read(&read(&self.path(name)), &Limits::default()).unwrap().owned.faces
    }

    /// Whether the file still holds the pet region and the foreign property as they were written.
    fn pet_kept(&self, name: &str) -> bool {
        let text = String::from_utf8_lossy(&read(&self.path(name))).into_owned();
        text.contains(PET) && text.contains("<zz:keep>other application</zz:keep>")
    }

    fn store_names(&mut self, on: bool) {
        self.s.change_settings(|st| {
            let mut p = st.people();
            p.store_names_in_photos = on;
            st.set_people(&p);
        });
    }
}

/// Another application writes `bytes` over `path` (off the owner thread).
fn other_app(path: &Path, bytes: Vec<u8>) {
    let path = path.to_path_buf();
    off_owner(move || {
        let id = dalica_fs::identity(&path).unwrap().unwrap();
        dalica_fs::replace(&path, &bytes, Expect::Unchanged(id)).unwrap();
    });
}

/// The regions of `a.jpg`: Ana and bruno named, a face without a name, and Rex the dog.
fn a_regions() -> Vec<String> {
    vec![
        region(Some("Ana"), "Face", r(ANA)),
        region(Some("bruno"), "Face", r(BRUNO)),
        region(None, "Face", r(NOBODY)),
        region(Some("Rex"), "Pet", r(REX)),
    ]
}

/// A photo whose file another application gave named faces shows its people after the first scan:
/// a name matched to a person the library has ignoring case, a new one created once for every
/// photo naming it, an unnamed face in Unnamed, the pet no face; no undo step, nothing written
/// back. A face another application adds to the file later comes in on the rescan, the others as
/// they were.
#[test]
fn regions_from_another_application_are_faces_and_people_after_the_first_scan() {
    let a = with_regions(1, &a_regions());
    let b = with_regions(2, &[region(Some("BRUNO"), "Face", r(BRUNO))]);
    let mut l = Lib::new(&[("a.jpg", a.clone()), ("b.jpg", b.clone())], None, |s| {
        s.execute("people.create", &json!({ "name": "ANA" })).unwrap();
    });
    let steps = l.s.undo_stack().len();
    let faces = l.faces("a.jpg");
    assert_eq!(faces.len(), 3, "{faces:?}");
    let (ana, bruno) = (l.person("ana"), l.person("bruno"));
    assert_eq!(l.s.library().people().count(), 2, "ANA is Ana; bruno is created once");
    assert!(close(&faces[0].rect, ANA) && faces[0].person == FacePerson::Person(ana.clone()));
    assert!(close(&faces[1].rect, BRUNO) && faces[1].person == FacePerson::Person(bruno.clone()));
    assert!(close(&faces[2].rect, NOBODY) && faces[2].person == FacePerson::Unnamed);
    assert_eq!(l.faces("b.jpg").iter().map(|f| f.person.clone()).collect::<Vec<_>>(), [FacePerson::Person(bruno.clone())]);
    assert_eq!(l.s.library().person(&bruno).unwrap().name, "bruno", "the first spelling read");
    assert_eq!(l.s.undo_stack().len(), steps, "the first scan's faces are no undo step");
    assert_eq!(l.s.pending_metadata_writes(), 0, "nothing goes back into the files");
    assert_eq!((read(&l.path("a.jpg")), read(&l.path("b.jpg"))), (a, b));

    // Another application adds Carla: the rescan brings her in, the faces Dalica had stay.
    let mut more = a_regions();
    more.push(region(Some("Carla"), "Face", r(CARLA)));
    other_app(&l.path("a.jpg"), with_regions(1, &more));
    l.s.start_verification();
    l.s.wait_idle();
    let after = l.faces("a.jpg");
    assert_eq!(after.len(), 4, "{after:?}");
    assert_eq!(after[..3], faces[..], "the faces Dalica had are as they were");
    assert!(close(&after[3].rect, CARLA) && after[3].person == FacePerson::Person(l.person("Carla")));
    assert_eq!(l.s.undo_stack().len(), steps);
}

/// With Store people names in photos on, naming a face writes it into the file as a region that
/// reads back; renaming the person rewrites it, undo writes the old name, and undoing the naming
/// takes the region out again. The pet region and the foreign property survive every write, and
/// the owner thread makes no file system call.
#[test]
fn naming_a_face_writes_a_region_that_renaming_and_undo_rewrite() {
    let sidecar = json!({ "dalica": { "version": 1 }, "photos": { "c.jpg": { "faces": [{ "rect": ANA }] } } });
    let mut l = Lib::new(&[("c.jpg", with_regions(3, &[region(Some("Rex"), "Pet", r(REX))]))], Some(sidecar), |_| {});
    let c = l.id("c.jpg");
    assert_eq!(l.faces("c.jpg").len(), 1, "the sidecar's face; the pet is none");
    assert!(l.file_faces("c.jpg").is_empty());
    l.store_names(true);
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    let named = l.run("faces.name", json!({ "faces": [format!("{c}.0")], "name": "Ana", "create": true }));
    assert_eq!(l.s.pending_metadata_writes(), 1, "one write queued");
    assert!(l.s.flush_metadata(WAIT));
    l.s.poll();
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "no fs call was refused on the owner thread");
    l.flush();
    let file = l.file_faces("c.jpg");
    assert!(file.len() == 1 && file[0].name.as_deref() == Some("Ana") && close(&file[0].rect, ANA), "{file:?}");
    assert!(l.pet_kept("c.jpg"));

    let ana = named["person"].clone();
    l.run("people.rename", json!({ "person": ana, "name": "Ana Costa" }));
    l.flush();
    assert_eq!(l.file_faces("c.jpg").iter().map(|f| f.name.clone()).collect::<Vec<_>>(), [Some("Ana Costa".to_string())]);
    l.run("edit.undo", json!({}));
    l.flush();
    assert_eq!(l.file_faces("c.jpg").iter().map(|f| f.name.clone()).collect::<Vec<_>>(), [Some("Ana".to_string())], "undo writes the old name");
    assert!(l.pet_kept("c.jpg"));
    l.run("edit.undo", json!({}));
    l.flush();
    assert!(l.file_faces("c.jpg").is_empty(), "an unnamed face is never written");
    assert!(l.pet_kept("c.jpg"), "the pet survives every write");
    assert_eq!(l.name_of(&l.faces("c.jpg")[0]), None);
}

/// With the option off a named face is written nowhere; Write People Names to Photos… asks first,
/// then writes the selection's named faces once (photos without faces are skipped), and turning
/// the option on writes nothing until a face changes.
#[test]
fn with_the_option_off_nothing_is_written_until_write_people_names_is_confirmed() {
    let sidecar = json!({ "dalica": { "version": 1 }, "photos": { "c.jpg": { "faces": [{ "rect": BRUNO }] } } });
    let files = [("c.jpg", with_regions(3, &[region(Some("Rex"), "Pet", r(REX))])), ("d.jpg", jpeg(4))];
    let mut l = Lib::new(&files, Some(sidecar), |_| {});
    let (c, d) = (l.id("c.jpg"), l.id("d.jpg"));
    let before = read(&l.path("c.jpg"));
    l.run("faces.name", json!({ "faces": [format!("{c}.0")], "name": "Bruno", "create": true }));
    assert_eq!(l.s.pending_metadata_writes(), 0, "nothing is queued");
    l.store_names(true);
    l.flush();
    assert_eq!(read(&l.path("c.jpg")), before, "turning the option on writes nothing by itself");
    l.store_names(false);

    let asked = l.s.execute("faces.writeXmp", &json!({ "ids": [c, d] }));
    assert_eq!(asked, Err(CmdError::Disabled(format!("{} Send \"confirm\": true to go ahead.", question(2)))));
    assert_eq!(l.s.pending_metadata_writes(), 0);
    let v = l.run("faces.writeXmp", json!({ "ids": [c, d], "confirm": true }));
    assert_eq!((v["queued"].clone(), v["skipped"].clone()), (json!(1), json!(1)));
    l.flush();
    let file = l.file_faces("c.jpg");
    assert!(file.len() == 1 && file[0].name.as_deref() == Some("Bruno") && close(&file[0].rect, BRUNO), "{file:?}");
    assert!(l.pet_kept("c.jpg"));
    assert_eq!(read(&l.path("d.jpg")), jpeg(4), "a photo without faces is not touched");
    for (params, code) in [
        (json!({ "ids": [] }), "disabled"),
        (json!({ "ids": [999_999], "confirm": true }), "not_found"),
        (json!({ "ids": "c", "confirm": true }), "bad_params"),
        (json!({ "ids": [c], "confirm": "yes" }), "bad_params"),
    ] {
        assert_eq!(l.s.execute("faces.writeXmp", &params).unwrap_err().code(), code, "{params}");
    }
}

/// A face another application added to the file since Dalica read it is brought into the library
/// before Dalica writes its people names, and is still in the file afterwards; a write is never
/// made over it.
#[test]
fn a_face_added_in_another_application_is_brought_in_before_a_write() {
    let sidecar = json!({ "dalica": { "version": 1 }, "photos": { "c.jpg": { "faces": [{ "rect": ANA }] } } });
    let mut l = Lib::new(&[("c.jpg", with_regions(3, &[region(Some("Rex"), "Pet", r(REX))]))], Some(sidecar), |_| {});
    let c = l.id("c.jpg");
    l.store_names(true);
    l.run("faces.name", json!({ "faces": [format!("{c}.0")], "name": "Ana", "create": true }));
    l.flush();
    // Another application names Carla in the file; before any rescan, Dalica renames Ana.
    let regions = [region(Some("Rex"), "Pet", r(REX)), region(Some("Ana"), "Face", r(ANA)), region(Some("Carla"), "Face", r(CARLA))];
    other_app(&l.path("c.jpg"), with_regions(3, &regions));
    let ana = l.person("Ana");
    l.run("people.rename", json!({ "person": ana, "name": "Ana Costa" }));
    l.flush();
    let faces = l.faces("c.jpg");
    assert_eq!(faces.iter().map(|f| l.name_of(f)).collect::<Vec<_>>(), [Some("Ana Costa".to_string()), Some("Carla".to_string())]);
    let mut file: Vec<Option<String>> = l.file_faces("c.jpg").into_iter().map(|f| f.name).collect();
    file.sort();
    assert_eq!(file, [Some("Ana Costa".to_string()), Some("Carla".to_string())], "Carla is still in the file, Ana renamed");
    assert!(l.file_faces("c.jpg").iter().any(|f| close(&f.rect, CARLA)));
    assert!(l.pet_kept("c.jpg"));
}
