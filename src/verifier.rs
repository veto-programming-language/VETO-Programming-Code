use crate::ast::*;
use std::collections::HashMap;

pub type Range = (i128, i128);
pub type EnvMap = HashMap<String, Range>;
pub type ArrayMap = HashMap<String, ArrayInfo>;
pub type SigMap = HashMap<String, FnSig>;

#[derive(Clone)]
pub struct FnSig {
    pub params: Vec<(String, i64, i64)>,
    pub ret_min: i64,
    pub ret_max: i64,
}

#[derive(Clone)]
pub struct ArrayInfo {
    pub len: usize,
    pub elem_min: i128,
    pub elem_max: i128,
}

pub fn eval_expr_range(
    expr: &Expr,
    env: &EnvMap,
    arrays: &ArrayMap,
    sigs: &SigMap,
) -> Result<Range, String> {
    let range = match expr {
        Expr::Number(n) => (*n as i128, *n as i128),
        Expr::Str(..) => (0, 0),
        Expr::UnaryNot(inner) => {
            let _ = eval_expr_range(inner, env, arrays, sigs)?;
            (0, 1)
        }
        Expr::Field(obj, field) => {
            let key = format!("{}.{}", obj, field);
            if let Some(&iv) = env.get(&key) {
                iv
            } else if let Some((_, &iv)) = env.iter().find(|(k, _)| k.ends_with(&format!(".{}", field))) {
                iv
            } else {
                return Err(format!("unknown field {}.{}", obj, field));
            }
        }
        Expr::StructInit(..) => (0, 0),
        Expr::EnumVal(en, var, payload) => {
            if let Some(pexpr) = payload {
                let (pmin, pmax) = eval_expr_range(pexpr, env, arrays, sigs)?;
                if let Some(&(cmin, cmax)) = env.get(&format!("__enum_payload_{}::{}", en, var)) {
                    if pmin < cmin || pmax > cmax {
                        return Err(format!(
                            "variant {}::{} requires range <{}..{}>, got [{}..{}]",
                            en, var, cmin, cmax, pmin, pmax
                        ));
                    }
                }
            }
            env.get(&format!("{}::{}", en, var))
                .copied()
                .ok_or_else(|| format!("unknown variant {}::{}", en, var))?
        }
        Expr::Var(v) => env
            .get(v)
            .copied()
            .ok_or_else(|| format!("variable {} has no range bounds", v))?,
        Expr::Index(arr_name, idx_expr) => {
            let info = arrays
                .get(arr_name)
                .ok_or_else(|| format!("unknown array {}", arr_name))?;
            let (idx_min, idx_max) = eval_expr_range(idx_expr, env, arrays, sigs)?;
            if idx_min < 0 || idx_max >= info.len as i128 {
                return Err(format!(
                    "out of bounds access on {}[0..{}]: [{}..{}]",
                    arr_name,
                    info.len - 1,
                    idx_min,
                    idx_max
                ));
            }
            (info.elem_min, info.elem_max)
        }
        Expr::MethodCall(obj, method, args) => {
            let found_sig = sigs
                .iter()
                .find(|(k, _)| k.ends_with(&format!("__{}", method)))
                .map(|(_, s)| s.clone());
            let sig = found_sig.ok_or_else(|| format!("unknown method {}.{}", obj, method))?;
            if args.len() + 1 != sig.params.len() {
                return Err(format!(
                    "method {}.{} expects {} arguments, got {}",
                    obj,
                    method,
                    sig.params.len().saturating_sub(1),
                    args.len()
                ));
            }
            for (arg_expr, (p_name, p_min, p_max)) in args.iter().zip(sig.params.iter().skip(1)) {
                if matches!(arg_expr, Expr::Str(..)) {
                    continue;
                }
                let (a_min, a_max) = eval_expr_range(arg_expr, env, arrays, sigs)?;
                if a_min < *p_min as i128 || a_max > *p_max as i128 {
                    return Err(format!(
                        "argument {} of {}.{} requires <{}..{}>, got [{}..{}]",
                        p_name, obj, method, p_min, p_max, a_min, a_max
                    ));
                }
            }
            (sig.ret_min as i128, sig.ret_max as i128)
        }
        Expr::Call(fn_name, args) => {
            let sig = sigs
                .get(fn_name)
                .ok_or_else(|| format!("unknown function {}", fn_name))?;
            if args.len() != sig.params.len() {
                return Err(format!(
                    "function {} expects {} arguments, got {}",
                    fn_name,
                    sig.params.len(),
                    args.len()
                ));
            }
            for (arg_expr, (p_name, p_min, p_max)) in args.iter().zip(sig.params.iter()) {
                if matches!(arg_expr, Expr::Str(..)) {
                    continue;
                }
                let (a_min, a_max) = eval_expr_range(arg_expr, env, arrays, sigs)?;
                if a_min < *p_min as i128 || a_max > *p_max as i128 {
                    return Err(format!(
                        "argument {} of {} requires <{}..{}>, got [{}..{}]",
                        p_name, fn_name, p_min, p_max, a_min, a_max
                    ));
                }
            }
            (sig.ret_min as i128, sig.ret_max as i128)
        }
        Expr::BinOp(l, op, r) => {
            let (l1, h1) = eval_expr_range(l, env, arrays, sigs)?;
            let (l2, h2) = eval_expr_range(r, env, arrays, sigs)?;
            match op.as_str() {
                "+" => (l1.saturating_add(l2), h1.saturating_add(h2)),
                "-" => (l1.saturating_sub(h2), h1.saturating_sub(l2)),
                "*" => {
                    let c = [
                        l1.saturating_mul(l2),
                        l1.saturating_mul(h2),
                        h1.saturating_mul(l2),
                        h1.saturating_mul(h2),
                    ];
                    (*c.iter().min().unwrap(), *c.iter().max().unwrap())
                }
                "/" => {
                    if l2 <= 0 && h2 >= 0 {
                        return Err(format!(
                            "possible division by zero! Divisor in [{}..{}]",
                            l2, h2
                        ));
                    }
                    let c = [l1 / l2, l1 / h2, h1 / l2, h1 / h2];
                    (*c.iter().min().unwrap(), *c.iter().max().unwrap())
                }
                "%" => {
                    if l2 <= 0 && h2 >= 0 {
                        return Err(format!("modulo by zero! Divisor in [{}..{}]", l2, h2));
                    }
                    let m = l2.abs().max(h2.abs()) - 1;
                    if l1 >= 0 {
                        (0, m)
                    } else {
                        (-m, m)
                    }
                }
                "<<" => {
                    if l2 < 0 || h2 > 30 {
                        return Err(format!("unsafe left shift [{}..{}]", l2, h2));
                    }
                    (l1 << l2, h1 << h2)
                }
                ">>" => {
                    if l2 < 0 || h2 > 30 {
                        return Err(format!("unsafe right shift [{}..{}]", l2, h2));
                    }
                    let c = [l1 >> l2, l1 >> h2, h1 >> l2, h1 >> h2];
                    (*c.iter().min().unwrap(), *c.iter().max().unwrap())
                }
                "&" => {
                    if l1 >= 0 && l2 >= 0 {
                        (0, h1.min(h2))
                    } else if l2 >= 0 {
                        (0, h2)
                    } else {
                        (i32::MIN as i128, i32::MAX as i128)
                    }
                }
                "|" | "^" => {
                    if l1 >= 0 && l2 >= 0 {
                        let m = h1.max(h2);
                        let hi = if m == 0 {
                            0
                        } else {
                            (1i128 << (128 - m.leading_zeros())) - 1
                        };
                        (0, hi)
                    } else {
                        (l1.min(l2), h1.max(h2).saturating_mul(2))
                    }
                }
                "<" | "<=" | ">" | ">=" | "==" | "!=" | "&&" | "||" => (0, 1),
                _ => return Err(format!("unknown operator {}", op)),
            }
        }
    };
    if range.0 < i32::MIN as i128 || range.1 > u32::MAX as i128 {
        return Err(format!(
            "possible integer overflow: [{}..{}]",
            range.0, range.1
        ));
    }
    Ok(range)
}

pub fn refine_envs(cond: &Expr, base_env: &EnvMap) -> (EnvMap, EnvMap) {
    let mut then_env = base_env.clone();
    let mut else_env = base_env.clone();
    if let Expr::BinOp(l, op, r) = cond {
        if let (Expr::BinOp(inner_l, bop, inner_r), Expr::Number(limit)) = (l.as_ref(), r.as_ref()) {
            if bop == "+" {
                let target_key = match inner_l.as_ref() {
                    Expr::Var(v) => Some(v.clone()),
                    Expr::Field(o, f) => Some(format!("{}.{}", o, f)),
                    _ => None,
                };
                if let Some(key) = target_key {
                    if let Some(&(lo, hi)) = base_env.get(&key) {
                        let empty_arr = ArrayMap::new();
                        let empty_sig = SigMap::new();
                        if let Ok((_, off_max)) = eval_expr_range(inner_r, base_env, &empty_arr, &empty_sig) {
                            let lim = *limit as i128;
                            match op.as_str() {
                                "<" => {
                                    then_env.insert(key, (lo, hi.min(lim - off_max - 1)));
                                    return (then_env, else_env);
                                }
                                "<=" => {
                                    then_env.insert(key, (lo, hi.min(lim - off_max)));
                                    return (then_env, else_env);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        if op == "&&" {
            let (t1, _) = refine_envs(l, base_env);
            let (t2, _) = refine_envs(r, &t1);
            return (t2, else_env);
        }
        let var_opt = match l.as_ref() {
            Expr::Var(v) => Some(v.clone()),
            Expr::Field(o, f) => Some(format!("{}.{}", o, f)),
            _ => None,
        };
        let r_var_opt = match r.as_ref() {
            Expr::Var(v) => Some(v.clone()),
            Expr::Field(o, f) => Some(format!("{}.{}", o, f)),
            _ => None,
        };
        if let (Some(v1), Some(v2)) = (&var_opt, &r_var_opt) {
            if let (Some(&(lo1, hi1)), Some(&(lo2, hi2))) = (base_env.get(v1), base_env.get(v2)) {
                match op.as_str() {
                    "<" => {
                        then_env.insert(v1.clone(), (lo1, hi1.min(hi2 - 1)));
                        then_env.insert(v2.clone(), (lo2.max(lo1 + 1), hi2));
                    }
                    "<=" => {
                        then_env.insert(v1.clone(), (lo1, hi1.min(hi2)));
                        then_env.insert(v2.clone(), (lo2.max(lo1), hi2));
                    }
                    ">" => {
                        then_env.insert(v1.clone(), (lo1.max(lo2 + 1), hi1));
                        then_env.insert(v2.clone(), (lo2, hi2.min(hi1 - 1)));
                    }
                    ">=" => {
                        then_env.insert(v1.clone(), (lo1.max(lo2), hi1));
                        then_env.insert(v2.clone(), (lo2, hi2.min(hi1)));
                    }
                    _ => {}
                }
            }
        }
        let norm = match (var_opt, l.as_ref(), r_var_opt, r.as_ref()) {
            (Some(v), _, _, Expr::Number(n)) => Some((v, op.as_str(), *n as i128)),
            (_, Expr::Number(n), Some(v), _) => {
                let flipped = match op.as_str() {
                    ">" => "<",
                    ">=" => "<=",
                    "<" => ">",
                    "<=" => ">=",
                    other => other,
                };
                Some((v, flipped, *n as i128))
            }
            _ => None,
        };
        if let Some((var, nop, c)) = norm {
            if let Some(&(lo, hi)) = base_env.get(&var) {
                match nop {
                    ">" => {
                        then_env.insert(var.clone(), (lo.max(c + 1), hi));
                        else_env.insert(var.clone(), (lo, hi.min(c)));
                    }
                    ">=" => {
                        then_env.insert(var.clone(), (lo.max(c), hi));
                        else_env.insert(var.clone(), (lo, hi.min(c - 1)));
                    }
                    "<" => {
                        then_env.insert(var.clone(), (lo, hi.min(c - 1)));
                        else_env.insert(var.clone(), (lo.max(c), hi));
                    }
                    "<=" => {
                        then_env.insert(var.clone(), (lo, hi.min(c)));
                        else_env.insert(var.clone(), (lo.max(c + 1), hi));
                    }
                    "==" => {
                        then_env.insert(var.clone(), (c, c));
                        if c == lo && lo < hi {
                            else_env.insert(var.clone(), (lo + 1, hi));
                        }
                        if c == hi && lo < hi {
                            else_env.insert(var.clone(), (lo, hi - 1));
                        }
                    }
                    "!=" => {
                        if c == lo && lo < hi {
                            then_env.insert(var.clone(), (lo + 1, hi));
                        }
                        if c == hi && lo < hi {
                            then_env.insert(var.clone(), (lo, hi - 1));
                        }
                        else_env.insert(var.clone(), (c, c));
                    }
                    _ => {}
                }
            }
        }
    }
    (then_env, else_env)
}
