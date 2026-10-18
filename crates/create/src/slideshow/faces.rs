//! Where Pan and Zoom moves (`viewing.md` §1.3, §1.6): toward the photo's largest named face, else
//! its largest face, else its centre. The caller gives the faces as they show on the photo
//! (normalised to the picture shown, after its crop and turns); this picks one and its centre
//! becomes the [`Slide`](super::Slide)'s focus.

use super::geometry::{Point, Rect, unit};

/// One face on a photo, as the photo shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FaceBox {
    /// Normalised to the picture shown (`0..1`, top-left origin): `x`, `y`, `width`, `height`.
    pub rect: Rect,
    /// Whether the face is a person's (named), rather than unnamed.
    pub named: bool,
}

impl FaceBox {
    /// Its area inside the picture; 0 for a face that is not finite or lies outside it.
    fn area(&self) -> f32 {
        let r = self.rect;
        let (x0, y0) = (unit(r.x), unit(r.y));
        let (x1, y1) = (unit(r.x + r.width), unit(r.y + r.height));
        let a = (x1 - x0) * (y1 - y0);
        if a.is_finite() && a > 0.0 { a } else { 0.0 }
    }

    /// Its centre, inside the picture.
    fn centre(&self) -> Point {
        let r = self.rect;
        let mid = |a: f32, b: f32| unit((unit(a) + unit(b)) / 2.0);
        Point::new(mid(r.x, r.x + r.width), mid(r.y, r.y + r.height))
    }
}

/// The face Pan and Zoom moves toward, by its place in `faces`: the largest named face (by its area
/// on the picture), else the largest face; the first of equals. `None` when no face shows.
pub fn focus_face(faces: &[FaceBox]) -> Option<usize> {
    let largest = |named: bool| {
        faces
            .iter()
            .enumerate()
            .filter(|(_, f)| (!named || f.named) && f.area() > 0.0)
            .fold(None::<(usize, f32)>, |best, (i, f)| if best.is_none_or(|(_, a)| f.area() > a) { Some((i, f.area())) } else { best })
            .map(|(i, _)| i)
    };
    largest(true).or_else(|| largest(false))
}

/// The point Pan and Zoom moves toward, normalised in the photo: the centre of
/// [`focus_face`]'s face, else the photo's centre.
pub fn face_focus(faces: &[FaceBox]) -> Point {
    focus_face(faces).and_then(|i| faces.get(i)).map_or(Point::CENTRE, FaceBox::centre)
}
