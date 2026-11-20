use crate::ast::*;

pub fn optimize_program(prog: &mut Program) {
    for func in &mut prog.functions {
        func.body = opt_stmts(&func.body);
    }
}

fn opt_stmts(stmts: &[Stmt]) -> StmtList {
    let mut out = Vec::with_capacity(stmts.len());
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                line,
                name,
                type_name,
                min_val,
                max_val,
                value,
            } => out.push(Stmt::Let {
                line: *line,
                name: name.clone(),
                type_name: type_name.clone(),
                min_val: *min_val,
                max_val: *max_val,
                value: opt_expr(value),
            }),
            Stmt::LetArray {
                line,
                name,
                min_val,
                max_val,
                len,
                elements,
            } => out.push(Stmt::LetArray {
                line: *line,
                name: name.clone(),
                min_val: *min_val,
                max_val: *max_val,
                len: *len,
                elements: elements.iter().map(opt_expr).collect(),
            }),
            Stmt::Assign { line, name, value } => out.push(Stmt::Assign {
                line: *line,
                name: name.clone(),
                value: opt_expr(value),
            }),
            Stmt::FieldAssign {
                line,
                obj,
                field,
                value,
            } => out.push(Stmt::FieldAssign {
                line: *line,
                obj: obj.clone(),
                field: field.clone(),
                value: opt_expr(value),
            }),
            Stmt::IndexAssign {
                line,
                name,
                index,
                value,
            } => out.push(Stmt::IndexAssign {
                line: *line,
                name: name.clone(),
                index: opt_expr(index),
                value: opt_expr(value),
            }),
            Stmt::If {
                line,
                cond,
                then_body,
                else_body,
            } => {
                let c = opt_expr(cond);
                let tb = opt_stmts(then_body);
                let eb = opt_stmts(else_body);
                if let Expr::Number(n) = c {
                    if n != 0 {
                        out.extend(tb);
                    } else {
                        out.extend(eb);
                    }
                } else {
                    out.push(Stmt::If {
                        line: *line,
                        cond: c,
                        then_body: tb,
                        else_body: eb,
                    });
                }
            }
            Stmt::While { line, cond, body } => {
                let c = opt_expr(cond);
                if let Expr::Number(0) = c {
                    continue;
                }
                out.push(Stmt::While {
                    line: *line,
                    cond: c,
                    body: opt_stmts(body),
                });
            }
            Stmt::For {
                line,
                var,
                start,
                end,
                body,
            } => {
                let s = opt_expr(start);
                let e = opt_expr(end);
                if let (Expr::Number(a), Expr::Number(b)) = (&s, &e) {
                    if a >= b {
                        continue;
                    }
                }
                out.push(Stmt::For {
                    line: *line,
                    var: var.clone(),
                    start: s,
                    end: e,
                    body: opt_stmts(body),
                });
            }
            Stmt::Loop { line, body } => out.push(Stmt::Loop {
                line: *line,
                body: opt_stmts(body),
            }),
            Stmt::Match { line, expr, arms } => out.push(Stmt::Match {
                line: *line,
                expr: opt_expr(expr),
                arms: arms
                    .iter()
                    .map(|a| MatchArm {
                        line: a.line,
                        pat: a.pat.clone(),
                        body: opt_stmts(&a.body),
                    })
                    .collect(),
            }),
            Stmt::ExprStmt { line, expr } => out.push(Stmt::ExprStmt {
                line: *line,
                expr: opt_expr(expr),
            }),
            Stmt::Return { line, expr } => {
                out.push(Stmt::Return {
                    line: *line,
                    expr: opt_expr(expr),
                });
                break;
            }
            Stmt::Break { line } => {
                out.push(Stmt::Break { line: *line });
                break;
            }
            Stmt::Continue { line } => {
                out.push(Stmt::Continue { line: *line });
                break;
            }
            Stmt::Prove { line, cond } => out.push(Stmt::Prove {
                line: *line,
                cond: cond.clone(),
            }),
            Stmt::InlineAsm {
                line,
                template,
                clobbers,
            } => out.push(Stmt::InlineAsm {
                line: *line,
                template: template.clone(),
                clobbers: clobbers.clone(),
            }),
        }
    }
    out
}

fn opt_expr(expr: &Expr) -> Expr {
    match expr {
        Expr::UnaryNot(inner) => match opt_expr(inner) {
            Expr::Number(n) => Expr::Number(if n == 0 { 1 } else { 0 }),
            other => Expr::UnaryNot(Box::new(other)),
        },
        Expr::Call(name, args) => Expr::Call(name.clone(), args.iter().map(opt_expr).collect()),
        Expr::Index(arr, idx) => Expr::Index(arr.clone(), Box::new(opt_expr(idx))),
        Expr::StructInit(st, fields) => Expr::StructInit(
            st.clone(),
            fields
                .iter()
                .map(|(k, v)| (k.clone(), opt_expr(v)))
                .collect(),
        ),
        Expr::EnumVal(en, var, payload) => Expr::EnumVal(
            en.clone(),
            var.clone(),
            payload.as_ref().map(|p| Box::new(opt_expr(p))),
        ),
        Expr::BinOp(l, op, r) => {
            let lo = opt_expr(l);
            let ro = opt_expr(r);

            if let (Expr::Number(a), Expr::Number(b)) = (&lo, &ro) {
                let folded = match op.as_str() {
                    "+" => a.checked_add(*b),
                    "-" => a.checked_sub(*b),
                    "*" => a.checked_mul(*b),
                    "/" if *b != 0 => a.checked_div(*b),
                    "%" if *b != 0 => a.checked_rem(*b),
                    "<<" if *b >= 0 && *b < 31 => Some(a << b),
                    ">>" if *b >= 0 && *b < 31 => Some(a >> b),
                    "&" => Some(a & b),
                    "|" => Some(a | b),
                    "^" => Some(a ^ b),
                    "==" => Some(if a == b { 1 } else { 0 }),
                    "!=" => Some(if a != b { 1 } else { 0 }),
                    "<" => Some(if a < b { 1 } else { 0 }),
                    "<=" => Some(if a <= b { 1 } else { 0 }),
                    ">" => Some(if a > b { 1 } else { 0 }),
                    ">=" => Some(if a >= b { 1 } else { 0 }),
                    "&&" => Some(if *a != 0 && *b != 0 { 1 } else { 0 }),
                    "||" => Some(if *a != 0 || *b != 0 { 1 } else { 0 }),
                    _ => None,
                };
                if let Some(val) = folded {
                    return Expr::Number(val);
                }
            }

            match (op.as_str(), &lo, &ro) {
                ("+", _, Expr::Number(0)) | ("-", _, Expr::Number(0)) => return lo,
                ("+", Expr::Number(0), _) => return ro,
                ("*", _, Expr::Number(1)) | ("/", _, Expr::Number(1)) => return lo,
                ("*", Expr::Number(1), _) => return ro,
                ("*", _, Expr::Number(0)) | ("*", Expr::Number(0), _) => return Expr::Number(0),
                ("*", _, Expr::Number(k)) if *k > 1 && (*k & (*k - 1)) == 0 => {
                    let shift = k.trailing_zeros() as i64;
                    return Expr::BinOp(Box::new(lo), "<<".into(), Box::new(Expr::Number(shift)));
                }
                ("*", Expr::Number(k), _) if *k > 1 && (*k & (*k - 1)) == 0 => {
                    let shift = k.trailing_zeros() as i64;
                    return Expr::BinOp(Box::new(ro), "<<".into(), Box::new(Expr::Number(shift)));
                }
                _ => {}
            }
            Expr::BinOp(Box::new(lo), op.clone(), Box::new(ro))
        }
        other => other.clone(),
    }
}
