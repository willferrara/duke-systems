#[cfg(test)]
pub use duke_core::hid_raw::max_betaflight_dps;
pub use duke_core::hid_raw::{apply_betaflight_rate, sample_betaflight_preview, AxisRateConfig};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeadzoneShape {
    Radial,
    Axial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CurveMode {
    Linear,
    Exponential,
    SCurve,
    DynamicPrecision,
    Bezier5Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GyroActivationMode {
    Off,
    AlwaysOn,
    HoldLt,
    HoldRt,
    TouchpadTouch,
    ToggleButton,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StickCurveConfig {
    pub inner_deadzone: f32,
    pub outer_deadzone: f32,
    pub anti_deadzone: f32,
    pub shape: DeadzoneShape,
    pub curve_mode: CurveMode,
    pub expo_power: f32,
    pub custom_points: [[f32; 2]; 5],
    pub center_offset_x: f32,
    pub center_offset_y: f32,
}

impl Default for StickCurveConfig {
    fn default() -> Self {
        Self {
            inner_deadzone: 0.05,
            outer_deadzone: 0.98,
            anti_deadzone: 0.0,
            shape: DeadzoneShape::Radial,
            curve_mode: CurveMode::Linear,
            expo_power: 1.5,
            custom_points: [
                [0.0, 0.0],
                [0.25, 0.16],
                [0.50, 0.44],
                [0.75, 0.78],
                [1.0, 1.0],
            ],
            center_offset_x: 0.0,
            center_offset_y: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerConfig {
    pub activation_threshold: f32,
    pub hair_trigger_digital: bool,
    pub invert: bool,
}

impl Default for TriggerConfig {
    fn default() -> Self {
        Self {
            activation_threshold: 0.15,
            hair_trigger_digital: false,
            invert: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GyroConfig {
    pub activation_mode: GyroActivationMode,
    pub sensitivity: f32,
    pub vertical_ratio: f32,
    pub noise_floor_dps: f32,
    pub smoothing: f32,
    pub invert_x: bool,
    pub invert_y: bool,
}

impl Default for GyroConfig {
    fn default() -> Self {
        Self {
            activation_mode: GyroActivationMode::HoldLt,
            sensitivity: 4.5,
            vertical_ratio: 0.75,
            noise_floor_dps: 1.5,
            smoothing: 0.25,
            invert_x: false,
            invert_y: false,
        }
    }
}

/// Evaluates normalized `[0.0, 1.0]` curve magnitude without heap allocation.
#[inline]
pub fn eval_stick_curve(u: f32, cfg: &StickCurveConfig) -> f32 {
    let x = u.clamp(0.0, 1.0);
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }

    match cfg.curve_mode {
        CurveMode::Linear => x,
        CurveMode::Exponential => x.powf(cfg.expo_power.clamp(0.3, 4.0)).clamp(0.0, 1.0),
        CurveMode::SCurve => {
            let k = cfg.expo_power.clamp(0.5, 4.0);
            let a = x.powf(k);
            let b = (1.0 - x).powf(k);
            (a / (a + b).max(1e-6)).clamp(0.0, 1.0)
        }
        CurveMode::DynamicPrecision => {
            // Micro-aim dampening in inner 45% transitioning into aggressive flick ramp
            let p = cfg.expo_power.clamp(0.5, 4.0);
            if x < 0.45 {
                let t = x / 0.45;
                (0.26 * t.powf(1.25 * (p * 0.65).max(0.6))).clamp(0.0, 1.0)
            } else {
                let t = (x - 0.45) / 0.55;
                let s = t * t * (3.0 - 2.0 * t);
                (0.26 + 0.74 * s.powf((1.0 / p).clamp(0.45, 1.4))).clamp(0.0, 1.0)
            }
        }
        CurveMode::Bezier5Point => eval_bezier_5point(x, &cfg.custom_points),
    }
}

/// Evaluates a 5-point piecewise-smooth curve passing cleanly through `[0..4]` control knots.
#[inline]
fn eval_bezier_5point(x: f32, pts: &[[f32; 2]; 5]) -> f32 {
    let mut sorted = *pts;
    sorted[0][0] = sorted[0][0].clamp(0.0, 1.0);
    sorted[0][1] = sorted[0][1].clamp(0.0, 1.0);
    for i in 1..5 {
        sorted[i][0] = sorted[i][0].clamp(sorted[i - 1][0], 1.0);
        sorted[i][1] = sorted[i][1].clamp(0.0, 1.0);
    }

    if x <= sorted[0][0] {
        return sorted[0][1];
    }
    if x >= sorted[4][0] {
        return sorted[4][1];
    }

    // Find active span and apply smooth cubic Hermite blend using local tangents
    let mut seg = 0usize;
    for i in 0..4 {
        if x >= sorted[i][0] && x <= sorted[i + 1][0] {
            seg = i;
            break;
        }
    }

    let x0 = sorted[seg][0];
    let y0 = sorted[seg][1];
    let x1 = sorted[seg + 1][0];
    let y1 = sorted[seg + 1][1];
    let dx = (x1 - x0).max(1e-5);
    let t = ((x - x0) / dx).clamp(0.0, 1.0);

    let m0 = if seg == 0 {
        (y1 - y0) / dx
    } else {
        let prev_dx = (x1 - sorted[seg - 1][0]).max(1e-5);
        (y1 - sorted[seg - 1][1]) / prev_dx
    };
    let m1 = if seg == 3 {
        (y1 - y0) / dx
    } else {
        let next_dx = (sorted[seg + 2][0] - x0).max(1e-5);
        (sorted[seg + 2][1] - y0) / next_dx
    };

    let t2 = t * t;
    let t3 = t2 * t;
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;

    (h00 * y0 + h10 * dx * m0 + h01 * y1 + h11 * dx * m1).clamp(0.0, 1.0)
}

#[inline]
fn shape_single_axis(val: f32, cfg: &StickCurveConfig) -> f32 {
    let mag = val.abs().min(1.0);
    let inner = cfg.inner_deadzone.clamp(0.0, 0.45);
    let outer = cfg.outer_deadzone.clamp((inner + 0.05).min(0.95), 1.0);
    if mag <= inner {
        return 0.0;
    }
    let norm = ((mag - inner) / (outer - inner).max(0.05)).clamp(0.0, 1.0);
    let curved = eval_stick_curve(norm, cfg);
    let anti = cfg.anti_deadzone.clamp(0.0, 0.45);
    let out_mag = if curved > 0.0001 {
        (anti + curved * (1.0 - anti)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    val.signum() * out_mag
}

/// Applies center drift offset, Radial/Axial deadzone, outer deadzone, curve, and anti-deadzone.
#[inline]
pub fn apply_stick_transform(raw_x: f32, raw_y: f32, cfg: &StickCurveConfig) -> (f32, f32) {
    let x = (raw_x - cfg.center_offset_x).clamp(-1.0, 1.0);
    let y = (raw_y - cfg.center_offset_y).clamp(-1.0, 1.0);

    match cfg.shape {
        DeadzoneShape::Axial => (shape_single_axis(x, cfg), shape_single_axis(y, cfg)),
        DeadzoneShape::Radial => {
            let mag = (x * x + y * y).sqrt();
            let inner = cfg.inner_deadzone.clamp(0.0, 0.45);
            let outer = cfg.outer_deadzone.clamp((inner + 0.05).min(0.95), 1.0);
            if mag <= inner || mag < 1e-6 {
                return (0.0, 0.0);
            }
            let norm = ((mag - inner) / (outer - inner).max(0.05)).clamp(0.0, 1.0);
            let curved = eval_stick_curve(norm, cfg);
            let anti = cfg.anti_deadzone.clamp(0.0, 0.45);
            let out_mag = if curved > 0.0001 {
                (anti + curved * (1.0 - anti)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let scale = out_mag / mag;
            ((x * scale).clamp(-1.0, 1.0), (y * scale).clamp(-1.0, 1.0))
        }
    }
}

/// Applies hair-trigger threshold and optional instant digital snap.
#[inline]
pub fn apply_trigger_transform(raw: f32, cfg: &TriggerConfig) -> f32 {
    let mut v = raw.clamp(0.0, 1.0);
    if cfg.invert {
        v = 1.0 - v;
    }
    let thr = cfg.activation_threshold.clamp(0.01, 1.0);
    if v < thr {
        0.0
    } else if cfg.hair_trigger_digital {
        1.0
    } else {
        ((v - thr) / (1.0 - thr).max(0.02)).clamp(0.0, 1.0)
    }
}

pub fn sample_curve_preview(cfg: &StickCurveConfig, steps: usize) -> Vec<[f32; 2]> {
    let n = steps.clamp(16, 128);
    let mut out = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let x = i as f32 / n as f32;
        let (y, _) = apply_stick_transform(x, 0.0, cfg);
        out.push([x, y.abs()]);
    }
    out
}

/// Zero-allocation stack state for sub-pixel gyro-to-mouse EMA filtering & remainder accumulation.
#[derive(Debug, Default, Clone, Copy)]
pub struct GyroFilterState {
    pub smooth_yaw: f32,
    pub smooth_pitch: f32,
    pub accum_x: f32,
    pub accum_y: f32,
    pub toggled_active: bool,
    pub prev_toggle_down: bool,
}

impl GyroFilterState {
    #[inline]
    pub fn update_toggle(&mut self, toggle_button_down: bool) {
        if toggle_button_down && !self.prev_toggle_down {
            self.toggled_active = !self.toggled_active;
        }
        self.prev_toggle_down = toggle_button_down;
    }

    #[inline]
    pub fn process(
        &mut self,
        yaw_dps: f32,
        pitch_dps: f32,
        active: bool,
        dt_sec: f32,
        cfg: &GyroConfig,
    ) -> (i32, i32) {
        if !active || cfg.activation_mode == GyroActivationMode::Off {
            self.smooth_yaw = 0.0;
            self.smooth_pitch = 0.0;
            self.accum_x = 0.0;
            self.accum_y = 0.0;
            return (0, 0);
        }

        let mag = (yaw_dps * yaw_dps + pitch_dps * pitch_dps).sqrt();
        let floor = cfg.noise_floor_dps.clamp(0.0, 25.0);
        let (clean_yaw, clean_pitch) = if mag <= floor || mag < 1e-5 {
            (0.0, 0.0)
        } else {
            let scale = (mag - floor) / mag;
            (yaw_dps * scale, pitch_dps * scale)
        };

        let alpha = (1.0 - cfg.smoothing.clamp(0.0, 0.95)).max(0.05);
        self.smooth_yaw += alpha * (clean_yaw - self.smooth_yaw);
        self.smooth_pitch += alpha * (clean_pitch - self.smooth_pitch);

        let dir_x = if cfg.invert_x { -1.0 } else { 1.0 };
        let dir_y = if cfg.invert_y { 1.0 } else { -1.0 };
        let counts_per_deg = cfg.sensitivity.clamp(0.1, 25.0) * 12.0;

        self.accum_x += self.smooth_yaw * dt_sec * counts_per_deg * dir_x;
        self.accum_y +=
            self.smooth_pitch * dt_sec * counts_per_deg * cfg.vertical_ratio.clamp(0.1, 2.5) * dir_y;

        let dx = self.accum_x.trunc() as i32;
        let dy = self.accum_y.trunc() as i32;
        self.accum_x -= dx as f32;
        self.accum_y -= dy as f32;

        (dx, dy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radial_deadzone_and_anti_deadzone() {
        let cfg = StickCurveConfig {
            inner_deadzone: 0.10,
            outer_deadzone: 0.90,
            anti_deadzone: 0.15,
            shape: DeadzoneShape::Radial,
            curve_mode: CurveMode::Linear,
            ..StickCurveConfig::default()
        };
        assert_eq!(apply_stick_transform(0.05, 0.05, &cfg), (0.0, 0.0));
        let (ox, oy) = apply_stick_transform(0.50, 0.0, &cfg);
        assert!(ox > 0.50 && ox < 0.65);
        assert!(oy.abs() < 1e-6);
        let (mx, _) = apply_stick_transform(0.95, 0.0, &cfg);
        assert!((mx - 1.0).abs() < 1e-5);
    }

    #[test]
    fn bezier_and_betaflight_math() {
        let cfg = StickCurveConfig {
            curve_mode: CurveMode::Bezier5Point,
            ..Default::default()
        };
        let pts = sample_curve_preview(&cfg, 32);
        assert_eq!(pts.len(), 33);
        assert!(pts[0][1] < 1e-5);
        assert!((pts[32][1] - 1.0).abs() < 1e-5);

        let rates = AxisRateConfig {
            rc_rate: 1.0,
            super_rate: 0.70,
            expo: 0.20,
        };
        let (norm, dps) = apply_betaflight_rate(1.0, &rates);
        assert!((norm - 1.0).abs() < 1e-4);
        assert!((dps - max_betaflight_dps(&rates)).abs() < 1e-2);
    }

    #[test]
    fn trigger_and_gyro_filter_accumulate_subpixels() {
        let trig = TriggerConfig {
            activation_threshold: 0.10,
            hair_trigger_digital: true,
            invert: false,
        };
        assert_eq!(apply_trigger_transform(0.05, &trig), 0.0);
        assert_eq!(apply_trigger_transform(0.12, &trig), 1.0);

        let gcfg = GyroConfig {
            activation_mode: GyroActivationMode::AlwaysOn,
            sensitivity: 2.0,
            vertical_ratio: 1.0,
            noise_floor_dps: 1.0,
            smoothing: 0.0,
            invert_x: false,
            invert_y: false,
        };
        let mut st = GyroFilterState::default();
        assert_eq!(st.process(0.5, 0.5, true, 0.01, &gcfg), (0, 0));
        let (dx, dy) = st.process(15.0, 0.0, true, 0.01, &gcfg);
        assert!(dx > 0);
        assert_eq!(dy, 0);
        st.update_toggle(true);
        assert!(st.toggled_active);
        st.update_toggle(false);
        st.update_toggle(true);
        assert!(!st.toggled_active);
    }
}
