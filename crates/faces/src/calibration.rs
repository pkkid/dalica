//! The Suggestion and Grouping confidence sliders (50 … 95 in steps of 5) as cosine thresholds of
//! the model in use (`docs/faces.md` §5).
//!
//! A built-in model carries a table of one cosine per step, measured so that a higher confidence
//! means fewer, more certain matches; its value at 85 is the model's recommended `matchCosine`. A
//! model without a table uses `matchCosine + (value − 85) × 0.01`.

/// The slider's steps.
pub const STEPS: [u8; 10] = [50, 55, 60, 65, 70, 75, 80, 85, 90, 95];

/// Where both sliders start.
pub const DEFAULT_CONFIDENCE: u8 = 85;

/// How much the threshold rises per point of confidence without a table.
const LINEAR_SLOPE: f32 = 0.01;

/// One of the slider's [`STEPS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Confidence(u8);

impl Confidence {
    /// `None` unless `value` is one of the [`STEPS`].
    pub fn new(value: u8) -> Option<Self> {
        STEPS.contains(&value).then_some(Self(value))
    }

    /// Every step, lowest first.
    pub fn all() -> impl Iterator<Item = Self> {
        STEPS.into_iter().map(Self)
    }

    pub fn value(self) -> u8 {
        self.0
    }

    /// The step's place in [`STEPS`].
    fn index(self) -> usize {
        STEPS.iter().position(|s| *s == self.0).unwrap_or(0)
    }
}

impl Default for Confidence {
    fn default() -> Self {
        Self(DEFAULT_CONFIDENCE)
    }
}

/// Why a calibration is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalibrationError {
    /// A value that is not a finite cosine (−1 … 1).
    NotACosine,
    /// A value lower than the one of the step before it: a higher confidence must never mean a
    /// lower threshold.
    Falls,
}

impl std::fmt::Display for CalibrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotACosine => write!(f, "a calibration value is not a cosine between -1 and 1"),
            Self::Falls => write!(f, "the calibration table falls as the confidence rises"),
        }
    }
}

impl std::error::Error for CalibrationError {}

/// A model's map from confidence to cosine threshold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Calibration {
    /// One cosine per step of [`STEPS`], never falling.
    Table([f32; 10]),
    /// `match_cosine + (value − 85) × 0.01`, kept within −1 … 1.
    Linear { match_cosine: f32 },
}

fn is_cosine(v: f32) -> bool {
    v.is_finite() && (-1.0..=1.0).contains(&v)
}

impl Calibration {
    /// A measured table (a built-in model's).
    pub fn table(values: [f32; 10]) -> Result<Self, CalibrationError> {
        if !values.iter().all(|v| is_cosine(*v)) {
            return Err(CalibrationError::NotACosine);
        }
        if values.windows(2).any(|w| matches!(w, [a, b] if b < a)) {
            return Err(CalibrationError::Falls);
        }
        Ok(Self::Table(values))
    }

    /// A model with only its recommended `matchCosine`.
    pub fn linear(match_cosine: f32) -> Result<Self, CalibrationError> {
        if is_cosine(match_cosine) { Ok(Self::Linear { match_cosine }) } else { Err(CalibrationError::NotACosine) }
    }

    /// The cosine threshold at `confidence`.
    pub fn threshold(&self, confidence: Confidence) -> f32 {
        match self {
            Self::Table(t) => t.get(confidence.index()).copied().unwrap_or(0.0),
            Self::Linear { match_cosine } => {
                let offset = (f32::from(confidence.value()) - f32::from(DEFAULT_CONFIDENCE)) * LINEAR_SLOPE;
                (match_cosine + offset).clamp(-1.0, 1.0)
            }
        }
    }
}
