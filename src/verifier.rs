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
            env.get(&key)
                .copied()
                .ok_or_else(|| format!("неизвестное поле {}.{}", obj, field))?
        }
        Expr::StructInit(..) => (0, 0),
        Expr::Var(v) => env
            .get(v)
            .copied()
            .ok_or_else(|| format!("у переменной {} не задан диапазон", v))?,
        Expr::Index(arr_name, idx_expr) => {
            let info = arrays
                .get(arr_name)
                .ok_or_else(|| format!("неизвестный массив {}", arr_name))?;
            let (idx_min, idx_max) = eval_expr_range(idx_expr, env, arrays, sigs)?;
            if idx_min < 0 || idx_max >= info.len as i128 {
                return Err(format!(
                    "выход за границы {}[0..{}]: [{}..{}]",
                    arr_name,
                    info.len - 1,
                    idx_min,
                    idx_max
                ));
            }
            (info.elem_min, info.elem_max)
        }
        Expr::Call(fn_name, args) => {
            let sig = sigs
                .get(fn_name)
                .ok_or_else(|| format!("неизвестная функция {}", fn_name))?;
            if args.len() != sig.params.len() {
                return Err(format!(
                    "функция {} ждёт {} аргументов, дано {}",
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
                        "аргумент {} в {} требует <{}..{}>, передан [{}..{}]",
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
                            "возможно деление на ноль! Делитель в [{}..{}]",
                            l2, h2
                        ));
                    }
                    let c = [l1 / l2, l1 / h2, h1 / l2, h1 / h2];
                    (*c.iter().min().unwrap(), *c.iter().max().unwrap())
                }
                "%" => {
                    if l2 <= 0 && h2 >= 0 {
                        return Err(format!(
                            "остаток от деления на ноль! Делитель в [{}..{}]",
                            l2, h2
                        ));
                    }
                    let m = l2.abs().max(h2.abs()) - 1;
                    (-m, m)
                }
                "<<" => {
                    if l2 < 0 || h2 > 30 {
                        return Err(format!("небезопасный сдвиг влево [{}..{}]", l2, h2));
                    }
                    (l1 << l2, h1 << h2)
                }
                ">>" => {
                    if l2 < 0 || h2 > 30 {
                        return Err(format!("небезопасный сдвиг вправо [{}..{}]", l2, h2));
                    }
                    let c = [l1 >> l2, l1 >> h2, h1 >> l2, h1 >> h2];
                    (*c.iter().min().unwrap(), *c.iter().max().unwrap())
                }
                "&" => {
                    if l2 >= 0 && h2 >= 0 {
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
                _ => return Err(format!("неизвестный оператор {}", op)),
            }
        }
    };
    if range.0 < i32::MIN as i128 || range.1 > i32::MAX as i128 {
        return Err(format!(
            "возможно переполнение i32: [{}..{}]",
            range.0, range.1
        ));
    }
    Ok(range)
}

pub fn refine_envs(cond: &Expr, base_env: &EnvMap) -> (EnvMap, EnvMap) {
    let mut then_env = base_env.clone();
    let mut else_env = base_env.clone();
    if let Expr::BinOp(l, op, r) = cond {
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
        if let (Some(var), Expr::Number(num)) = (var_opt, r.as_ref()) {
            let var = &var;
            if let Some(&(lo, hi)) = base_env.get(var) {
                let c = *num as i128;
                match op.as_str() {
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
                    }
                    _ => {}
                }
            }
        }
    }
    (then_env, else_env)
}
