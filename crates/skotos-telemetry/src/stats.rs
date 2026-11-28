//! Pure-Rust Statistical Inference Engine for Skotos Experimentation & Guarded Rollouts.
//!
//! Computes:
//! - Sample Ratio Mismatch (SRM) via Pearson Chi-Square + regularized upper incomplete gamma p-value
//! - Frequentist inference (two-proportion z-test for binary metrics, Welch's t-test for continuous)
//! - Bayesian inference (Normal-Normal posterior probability to beat control + credible intervals)
//! - Guarded Rollout regression detection (respecting `HigherIsBetter` vs `LowerIsBetter`)
//! - Mutual Exclusion Layer deterministic SHA-1 60-bit bucketing (`0..100_000`)

use sha1::{Digest, Sha1};
use skotos_domain::{
    GuardrailMetricCheck, GuardrailMetricRule, MetricDefinition, MetricExperimentResult,
    MetricSuccessDirection, SrmCheckResult, StatisticalEngine, VariationResult,
    VariationSufficientStats,
};

const SRM_ALPHA: f64 = 0.001;

/// Deterministic 60-bit SHA-1 layer bucket in `0..100_000` for Mutual Exclusion Layers.
pub fn layer_bucket(layer_key: &str, salt: &str, context_key: &str) -> i32 {
    let mut hasher = Sha1::new();
    hasher.update(format!("{layer_key}.{salt}.{context_key}").as_bytes());
    let digest = hasher.finalize();
    // Top 60 bits matching `crates/skotos-eval`
    let mut top64 = [0u8; 8];
    top64.copy_from_slice(&digest[..8]);
    let top60 = u64::from_be_bytes(top64) >> 4;
    let bucket = (u128::from(top60) * 100_000u128) / (1u128 << 60);
    bucket as i32
}

/// Standard Normal CDF `Phi(z)` using Abramowitz & Stegun 7.1.26 (`|err| < 1.5e-7`).
pub fn normal_cdf(z: f64) -> f64 {
    0.5 * erfc_approx(-z / std::f64::consts::SQRT_2)
}

/// Complementary error function `erfc(x)` accurate to ~1.5e-7 everywhere.
fn erfc_approx(x: f64) -> f64 {
    let ax = x.abs();
    let t = 1.0 / (1.0 + 0.3275911 * ax);
    let poly = t
        * (0.254829592
            + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    let erfc_pos = poly * (-ax * ax).exp();
    if x >= 0.0 {
        erfc_pos
    } else {
        2.0 - erfc_pos
    }
}

/// Inverse standard normal CDF `Phi^-1(p)` (Peter Acklam rational approximation, `< 1.15e-9`).
pub fn normal_ppf(p: f64) -> f64 {
    let p = p.clamp(1e-12, 1.0 - 1e-12);
    const A: [f64; 6] = [
        -3.969683028665376e+01,
        2.209460984245205e+02,
        -2.759285104469687e+02,
        1.383_577_518_672_69e2,
        -3.066479806614716e+01,
        2.506628277459239e+00,
    ];
    const B: [f64; 5] = [
        -5.447609879822406e+01,
        1.615858368580409e+02,
        -1.556989798598866e+02,
        6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    const C: [f64; 6] = [
        -7.784894002430293e-03,
        -3.223964580411365e-01,
        -2.400758277161838e+00,
        -2.549732539343734e+00,
        4.374664141464968e+00,
        2.938163982698783e+00,
    ];
    const D: [f64; 4] = [
        7.784695709041462e-03,
        3.224671290700398e-01,
        2.445134137142996e+00,
        3.754408661907416e+00,
    ];
    let p_low = 0.02425;
    let p_high = 1.0 - p_low;

    if p < p_low {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= p_high {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    }
}

/// Log-Gamma via Lanczos approximation (`g = 7`, 9 terms).
fn ln_gamma(z: f64) -> f64 {
    const COEFFS: [f64; 9] = [
        0.9999999999998099,
        676.5203681218851,
        -1259.1392167224028,
        771.3234287776531,
        -176.6150291621406,
        12.507343278686905,
        -0.13857109526572012,
        9.984369578019572e-6,
        1.5056327351493116e-7,
    ];
    let z = z - 1.0;
    let mut x = COEFFS[0];
    for (i, &c) in COEFFS.iter().enumerate().skip(1) {
        x += c / (z + i as f64);
    }
    let t = z + 7.5;
    0.5 * (2.0 * std::f64::consts::PI).ln() + (z + 0.5) * t.ln() - t + x.ln()
}

/// Regularized upper incomplete gamma `Q(a, x)` for chi-square survival `P(Chi2 >= x)`.
fn gamma_q(a: f64, x: f64) -> f64 {
    if x <= 0.0 || a <= 0.0 {
        return 1.0;
    }
    let gln = ln_gamma(a);
    if x < a + 1.0 {
        // Series representation for P(a, x), then Q = 1 - P
        let mut ap = a;
        let mut sum = 1.0 / a;
        let mut del = sum;
        for _ in 0..200 {
            ap += 1.0;
            del *= x / ap;
            sum += del;
            if del.abs() < sum.abs() * 1e-12 {
                break;
            }
        }
        let p = sum * (-x + a * x.ln() - gln).exp();
        (1.0 - p).clamp(0.0, 1.0)
    } else {
        // Lentz continued fraction for Q(a, x)
        let mut b = x + 1.0 - a;
        let mut c = 1.0 / 1e-30;
        let mut d = 1.0 / b;
        let mut h = d;
        for i in 1..=200 {
            let an = -(i as f64) * ((i as f64) - a);
            b += 2.0;
            d = an * d + b;
            if d.abs() < 1e-30 {
                d = 1e-30;
            }
            c = b + an / c;
            if c.abs() < 1e-30 {
                c = 1e-30;
            }
            d = 1.0 / d;
            let del = d * c;
            h *= del;
            if (del - 1.0).abs() < 1e-12 {
                break;
            }
        }
        ((-x + a * x.ln() - gln).exp() * h).clamp(0.0, 1.0)
    }
}

/// Upper-tail Chi-Square p-value `P(Chi^2_df >= chi_sq)`.
pub fn chi_square_p_value(chi_sq: f64, df: usize) -> f64 {
    if df == 0 || chi_sq <= 0.0 {
        return 1.0;
    }
    gamma_q(df as f64 / 2.0, chi_sq / 2.0)
}

/// Evaluates Sample Ratio Mismatch (SRM) across variations using Pearson Chi-Square.
pub fn check_srm(observed: &[i64], expected_weights: &[i32]) -> SrmCheckResult {
    let total_obs: i64 = observed.iter().sum();
    let total_weight: f64 = expected_weights
        .iter()
        .map(|w| f64::from((*w).max(0)))
        .sum();
    if observed.len() < 2
        || observed.len() != expected_weights.len()
        || total_obs < 20
        || total_weight <= 0.0
    {
        return SrmCheckResult {
            chi_square: 0.0,
            p_value: 1.0,
            srm_detected: false,
            expected_counts: expected_weights
                .iter()
                .map(|w| {
                    if total_weight > 0.0 {
                        (total_obs as f64) * f64::from(*w) / total_weight
                    } else {
                        0.0
                    }
                })
                .collect(),
            observed_counts: observed.to_vec(),
        };
    }

    let mut chi_sq = 0.0;
    let mut expected_counts = Vec::with_capacity(observed.len());
    for (&obs, &w) in observed.iter().zip(expected_weights.iter()) {
        let exp = (total_obs as f64) * f64::from(w.max(0)) / total_weight;
        expected_counts.push(exp);
        if exp > 0.0 {
            let diff = (obs as f64) - exp;
            chi_sq += (diff * diff) / exp;
        }
    }
    let df = observed.len() - 1;
    let p_value = chi_square_p_value(chi_sq, df);
    SrmCheckResult {
        chi_square: chi_sq,
        p_value,
        srm_detected: p_value < SRM_ALPHA,
        expected_counts,
        observed_counts: observed.to_vec(),
    }
}

fn mean_and_variance(metric: &MetricDefinition, s: &VariationSufficientStats) -> (f64, f64) {
    let n = s.exposures as f64;
    if n <= 0.0 {
        return (0.0, 0.0);
    }
    if metric.kind.is_binary() {
        let p = (s.conversions as f64 / n).clamp(0.0, 1.0);
        (p, p * (1.0 - p))
    } else {
        let mean = s.sum / n;
        let var = if n > 1.0 {
            ((s.sum_sq - n * mean * mean) / (n - 1.0)).max(0.0)
        } else {
            0.0
        };
        (mean, var)
    }
}

/// Evaluates an experiment metric across all variations using Frequentist or Bayesian statistics.
pub fn evaluate_metric_experiment(
    metric: &MetricDefinition,
    stats: &[VariationSufficientStats],
    control_variation_index: i32,
    engine: StatisticalEngine,
    alpha: f64,
) -> MetricExperimentResult {
    let alpha = alpha.clamp(0.001, 0.25);
    let z_crit = normal_ppf(1.0 - alpha / 2.0);
    let control = stats
        .iter()
        .find(|s| s.variation_index == control_variation_index)
        .cloned()
        .unwrap_or_else(|| VariationSufficientStats {
            variation_index: control_variation_index,
            ..Default::default()
        });
    let (ctrl_mean, ctrl_var) = mean_and_variance(metric, &control);
    let ctrl_se = if control.exposures > 0 {
        (ctrl_var / control.exposures as f64).sqrt()
    } else {
        0.0
    };

    let higher_is_better = metric.success_direction == MetricSuccessDirection::HigherIsBetter;
    let mut variations = Vec::with_capacity(stats.len());

    for s in stats {
        let is_control = s.variation_index == control_variation_index;
        let (mean, var) = mean_and_variance(metric, s);
        let se = if s.exposures > 0 {
            (var / s.exposures as f64).sqrt()
        } else {
            0.0
        };

        if is_control {
            variations.push(VariationResult {
                variation_index: s.variation_index,
                is_control: true,
                exposures: s.exposures,
                conversions: s.conversions,
                mean,
                std_err: se,
                relative_lift: Some(0.0),
                ci_lower: Some(0.0),
                ci_upper: Some(0.0),
                p_value: None,
                probability_to_beat_control: None,
                statistically_significant: false,
                winner: false,
            });
            continue;
        }

        let diff = mean - ctrl_mean;
        let se_diff = (ctrl_se * ctrl_se + se * se).sqrt();
        let rel_lift = if ctrl_mean.abs() > 1e-12 {
            diff / ctrl_mean.abs()
        } else {
            0.0
        };

        let (p_val, ci_low, ci_high, prob_beats) =
            if se_diff > 1e-12 && control.exposures >= 5 && s.exposures >= 5 {
                let z = diff / se_diff;
                let two_sided_p = (2.0 * (1.0 - normal_cdf(z.abs()))).clamp(0.0, 1.0);
                // Posterior P(Treatment beats Control) under weakly informative Normal prior
                let p_gt = normal_cdf(z);
                let prob_beats = if higher_is_better { p_gt } else { 1.0 - p_gt };
                let (low, high) = if ctrl_mean.abs() > 1e-12 {
                    (
                        (diff - z_crit * se_diff) / ctrl_mean.abs(),
                        (diff + z_crit * se_diff) / ctrl_mean.abs(),
                    )
                } else {
                    (diff - z_crit * se_diff, diff + z_crit * se_diff)
                };
                (Some(two_sided_p), Some(low), Some(high), Some(prob_beats))
            } else {
                (Some(1.0), Some(0.0), Some(0.0), Some(0.5))
            };

        let improved = if higher_is_better {
            diff > 0.0
        } else {
            diff < 0.0
        };

        let (sig, winner) = match engine {
            StatisticalEngine::Frequentist => {
                let s_flag = p_val.is_some_and(|p| p < alpha);
                (s_flag, s_flag && improved)
            }
            StatisticalEngine::Bayesian => {
                let win = prob_beats.is_some_and(|pb| pb >= 1.0 - alpha);
                let lose = prob_beats.is_some_and(|pb| pb <= alpha);
                (win || lose, win)
            }
        };

        variations.push(VariationResult {
            variation_index: s.variation_index,
            is_control: false,
            exposures: s.exposures,
            conversions: s.conversions,
            mean,
            std_err: se,
            relative_lift: Some(rel_lift),
            ci_lower: ci_low,
            ci_upper: ci_high,
            p_value: p_val,
            probability_to_beat_control: prob_beats,
            statistically_significant: sig,
            winner,
        });
    }

    MetricExperimentResult {
        metric_key: metric.key.clone(),
        metric_name: metric.name.clone(),
        kind: metric.kind,
        success_direction: metric.success_direction,
        variations,
    }
}

/// Evaluates a single Guarded Rollout metric rule to detect statistically significant regression.
pub fn evaluate_guardrail_metric(
    metric: &MetricDefinition,
    rule: &GuardrailMetricRule,
    control: &VariationSufficientStats,
    treatment: &VariationSufficientStats,
) -> GuardrailMetricCheck {
    let (ctrl_mean, ctrl_var) = mean_and_variance(metric, control);
    let (treat_mean, treat_var) = mean_and_variance(metric, treatment);

    let higher_is_better = metric.success_direction == MetricSuccessDirection::HigherIsBetter;
    // Positive regression_pct means treatment is WORSE than control
    let regression_pct = if ctrl_mean.abs() > 1e-12 {
        let delta_pct = ((treat_mean - ctrl_mean) / ctrl_mean.abs()) * 100.0;
        if higher_is_better {
            -delta_pct
        } else {
            delta_pct
        }
    } else if !higher_is_better && treat_mean > 0.0 {
        // Control had 0% errors/latency baseline and treatment introduced errors!
        treat_mean * 100.0
    } else {
        0.0
    };

    let se_diff = if control.exposures > 0 && treatment.exposures > 0 {
        (ctrl_var / control.exposures as f64 + treat_var / treatment.exposures as f64).sqrt()
    } else {
        0.0
    };
    let p_value = if se_diff > 1e-12 {
        let z = (treat_mean - ctrl_mean).abs() / se_diff;
        Some((2.0 * (1.0 - normal_cdf(z))).clamp(0.0, 1.0))
    } else {
        None
    };

    let enough_samples =
        control.exposures >= rule.min_sample_size && treatment.exposures >= rule.min_sample_size;
    let stat_sig = p_value.is_some_and(|p| p < 0.05);
    let regressed = enough_samples && stat_sig && regression_pct >= rule.max_regression_pct;

    GuardrailMetricCheck {
        metric_key: metric.key.clone(),
        control_mean: ctrl_mean,
        treatment_mean: treat_mean,
        control_samples: control.exposures,
        treatment_samples: treatment.exposures,
        regression_pct,
        p_value,
        regressed,
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use skotos_domain::{MetricAggregation, MetricKind};
    use uuid::Uuid;

    use super::*;

    fn sample_metric(kind: MetricKind, dir: MetricSuccessDirection) -> MetricDefinition {
        MetricDefinition {
            id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            key: "checkout_conv".to_string(),
            name: "Checkout Conversion".to_string(),
            description: String::new(),
            kind,
            event_key: "checkout.completed".to_string(),
            success_direction: dir,
            aggregation: MetricAggregation::Average,
            unit: "%".to_string(),
            randomization_unit: "user".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn detects_srm_when_allocation_skews_heavily() {
        let balanced = check_srm(&[5010, 4990], &[50_000, 50_000]);
        assert!(!balanced.srm_detected);
        assert!(balanced.p_value > 0.5);

        let skewed = check_srm(&[5400, 4600], &[50_000, 50_000]);
        assert!(skewed.srm_detected);
        assert!(skewed.p_value < 0.001);
    }

    #[test]
    fn frequentist_and_bayesian_detect_winning_conversion_treatment() {
        let metric = sample_metric(
            MetricKind::Conversion,
            MetricSuccessDirection::HigherIsBetter,
        );
        let stats = vec![
            VariationSufficientStats {
                variation_index: 0,
                exposures: 1000,
                conversions: 100,
                sum: 100.0,
                sum_sq: 100.0,
            },
            VariationSufficientStats {
                variation_index: 1,
                exposures: 1000,
                conversions: 160,
                sum: 160.0,
                sum_sq: 160.0,
            },
        ];

        let freq =
            evaluate_metric_experiment(&metric, &stats, 0, StatisticalEngine::Frequentist, 0.05);
        assert!(freq.variations[1].statistically_significant);
        assert!(freq.variations[1].winner);
        assert!(freq.variations[1].p_value.unwrap() < 0.001);

        let bayes =
            evaluate_metric_experiment(&metric, &stats, 0, StatisticalEngine::Bayesian, 0.05);
        assert!(bayes.variations[1].winner);
        assert!(bayes.variations[1].probability_to_beat_control.unwrap() > 0.999);
    }

    #[test]
    fn guardrail_detects_error_rate_regression() {
        let metric = sample_metric(MetricKind::ErrorRate, MetricSuccessDirection::LowerIsBetter);
        let rule = GuardrailMetricRule {
            metric_key: "error_rate".to_string(),
            max_regression_pct: 20.0,
            min_sample_size: 50,
        };
        let control = VariationSufficientStats {
            variation_index: 0,
            exposures: 200,
            conversions: 4, // 2% error rate
            sum: 4.0,
            sum_sq: 4.0,
        };
        let treatment = VariationSufficientStats {
            variation_index: 1,
            exposures: 200,
            conversions: 30, // 15% error rate -> +650% regression!
            sum: 30.0,
            sum_sq: 30.0,
        };
        let check = evaluate_guardrail_metric(&metric, &rule, &control, &treatment);
        assert!(check.regressed);
        assert!(check.regression_pct > 100.0);
    }

    #[test]
    fn mutual_exclusion_layer_bucketing_is_deterministic_and_bounded() {
        let b1 = layer_bucket("checkout-layer", "salt-1", "usr_123");
        let b2 = layer_bucket("checkout-layer", "salt-1", "usr_123");
        assert_eq!(b1, b2);
        assert!((0..100_000).contains(&b1));
    }
}
