use crate::expression::{Expression, split_formulas};
use anyhow::{Result, ensure};

pub const SAMPLES: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub xmin: f64,
    pub xmax: f64,
    pub ymin: f64,
    pub ymax: f64,
}

impl Viewport {
    pub fn new(xmin: f64, xmax: f64) -> Result<Self> {
        let view = Self {
            xmin,
            xmax,
            ymin: -20.0,
            ymax: 20.0,
        };
        ensure!(
            view.valid(),
            "Range endpoints must be finite, increasing, and have a finite span"
        );
        Ok(view)
    }
    pub fn valid(self) -> bool {
        [self.xmin, self.xmax, self.ymin, self.ymax]
            .iter()
            .all(|v| v.is_finite())
            && (self.xmax - self.xmin).is_finite()
            && (self.ymax - self.ymin).is_finite()
            && self.xmax > self.xmin
            && self.ymax > self.ymin
    }
    pub fn zoom(self, factor: f64, x: f64, y: f64) -> Self {
        let view = Self {
            xmin: x + (self.xmin - x) * factor,
            xmax: x + (self.xmax - x) * factor,
            ymin: y + (self.ymin - y) * factor,
            ymax: y + (self.ymax - y) * factor,
        };
        if view.valid()
            && view.xmax - view.xmin > 1e-12 * x.abs().max(1.0)
            && view.ymax - view.ymin > 1e-12 * y.abs().max(1.0)
        {
            view
        } else {
            self
        }
    }
    pub fn pan(self, x: f64, y: f64) -> Self {
        let view = Self {
            xmin: self.xmin + x,
            xmax: self.xmax + x,
            ymin: self.ymin + y,
            ymax: self.ymax + y,
        };
        if view.valid() { view } else { self }
    }
}

#[derive(Clone)]
pub struct Curve {
    pub label: String,
    pub expression: Expression,
}

#[derive(Clone)]
pub struct Plot {
    pub curves: Vec<Curve>,
    pub warnings: Vec<String>,
}

impl Plot {
    pub fn parse(text: &str) -> Result<Self> {
        let mut plot = Self {
            curves: Vec::new(),
            warnings: Vec::new(),
        };
        for formula in split_formulas(text) {
            if plot.curves.len() == 16 {
                plot.warnings
                    .push("Up to 16 curves can be shown; remaining formulas were skipped".into());
                break;
            }
            match Expression::parse(formula) {
                Ok(expression) => plot.curves.push(Curve {
                    label: formula.to_owned(),
                    expression,
                }),
                Err(error) => plot
                    .warnings
                    .push(format!("Error parsing '{formula}': {error}")),
            }
        }
        ensure!(
            !plot.curves.is_empty(),
            "{}",
            if plot.warnings.is_empty() {
                "Provide at least one formula".into()
            } else {
                plot.warnings.join("\n")
            }
        );
        Ok(plot)
    }
    pub fn fit_y(&self, mut view: Viewport) -> Viewport {
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        let mut stack = Vec::new();
        for curve in &self.curves {
            for i in 0..SAMPLES {
                let x = view.xmin + (view.xmax - view.xmin) * (i as f64 / (SAMPLES - 1) as f64);
                let y = curve.expression.evaluate_with_stack(x, &mut stack);
                if y.is_finite() {
                    min = min.min(y);
                    max = max.max(y);
                }
            }
        }
        if min.is_finite() && max.is_finite() {
            let padding = if max > min {
                (max - min) * 0.08
            } else {
                min.abs().max(1.0) * 0.1
            };
            let candidate = Viewport {
                ymin: min - padding,
                ymax: max + padding,
                ..view
            };
            if candidate.valid() {
                view = candidate;
            }
        }
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_default_y_limits_and_custom_x_range() {
        assert_eq!(
            Viewport::new(-3.0, 4.0).unwrap(),
            Viewport {
                xmin: -3.0,
                xmax: 4.0,
                ymin: -20.0,
                ymax: 20.0
            }
        );
    }
    #[test]
    fn rejects_non_finite_reversed_and_overflowing_ranges() {
        for (a, b) in [
            (1.0, 1.0),
            (2.0, -2.0),
            (f64::NAN, 1.0),
            (0.0, f64::INFINITY),
            (-f64::MAX, f64::MAX),
        ] {
            assert!(Viewport::new(a, b).is_err());
        }
    }
    #[test]
    fn bad_formula_does_not_hide_other_curves() {
        let p = Plot::parse("sin(x), unknown(x), 3").unwrap();
        assert_eq!(p.curves.len(), 2);
        assert_eq!(p.warnings.len(), 1);
        assert!(Plot::parse("unknown(x)").is_err());
    }
    #[test]
    fn zoom_keeps_anchor_fixed_and_reset_bounds_can_be_retained() {
        let v = Viewport::new(-10.0, 10.0).unwrap();
        let z = v.zoom(0.5, 0.0, 0.0);
        assert_eq!((z.xmin, z.xmax, z.ymin, z.ymax), (-5.0, 5.0, -10.0, 10.0));
        assert_eq!(z.pan(1.0, 2.0).xmin, -4.0);
    }
    #[test]
    fn fit_y_handles_constants_and_undefined_domains() {
        let v = Viewport::new(-10.0, 10.0).unwrap();
        let constant = Plot::parse("3").unwrap().fit_y(v);
        assert!(constant.ymin < 3.0 && constant.ymax > 3.0);
        assert_eq!(Plot::parse("sqrt(-1)").unwrap().fit_y(v), v);
    }
}
