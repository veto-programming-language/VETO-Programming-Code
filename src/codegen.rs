use crate::ast::*;
use crate::error::{ErrorBag, Stage};
use crate::verifier::{eval_expr_range, refine_envs, ArrayInfo, FnSig};
use std::collections::HashMap;

fn block_always_returns(stmts: &[Stmt]) -> bool {
    match stmts.last() {
        Some(Stmt::Return { .. }) => true,
        Some(Stmt::If {
            then_body,
            else_body,
            ..
        }) => {
            !else_body.is_empty()
                && block_always_returns(then_body)
                && block_always_returns(else_body)
        }
        _ => false,
    }
}

fn expr_to_c(expr: &Expr) -> String {
    match expr {
        Expr::Number(n) => n.to_string(),
        Expr::Str(s) => format!("\"{}\"", s.replace('"', "\\\"")),
        Expr::Var(v) => v.clone(),
        Expr::EnumVal(en, var, payload) => match payload {
            Some(e) => format!(
                "(struct {}){{ .tag = {}_{}, .payload = {} }}",
                en,
                en,
                var,
                expr_to_c(e)
            ),
            None => format!("{}_{}", en, var),
        },
        Expr::Field(obj, f) => format!("{}.{}", obj, f),
        Expr::StructInit(st, inits) => {
            let fs: StrList = inits
                .iter()
                .map(|(k, v)| format!(".{} = {}", k, expr_to_c(v)))
                .collect();
            format!("(struct {}){{ {} }}", st, fs.join(", "))
        }
        Expr::Index(arr, idx) => format!("{}[{}]", arr, expr_to_c(idx)),
        Expr::UnaryNot(inner) => format!("(!{})", expr_to_c(inner)),
        Expr::MethodCall(obj, method, args) => {
            let mut full_args = vec![if obj.ends_with("Arena") {
                String::from(char::from(38)) + obj
            } else {
                obj.clone()
            }];
            full_args.extend(args.iter().map(expr_to_c));
            format!("__METHOD__{}({})", method, full_args.join(", "))
        }
        Expr::Call(name, args) => {
            let args_c: StrList = args.iter().map(expr_to_c).collect();
            format!("{}({})", name, args_c.join(", "))
        }
        Expr::BinOp(l, op, r) => format!("({} {} {})", expr_to_c(l), op, expr_to_c(r)),
    }
}

fn check_execution_contexts(
    program: &Program,
    stmts: &[Stmt],
    func: &Function,
    file: &str,
    bag: &mut ErrorBag,
) {
    let is_irq = func
        .attrs
        .iter()
        .any(|a| a == "interrupt" || a == "irq_safe");
    let is_no_alloc = is_irq || func.attrs.iter().any(|a| a == "no_alloc");
    let is_lock_free = is_irq || func.attrs.iter().any(|a| a == "lock_free");
    if !is_irq && !is_no_alloc && !is_lock_free {
        return;
    }

    fn walk_expr(
        program: &Program,
        expr: &Expr,
        caller: &Function,
        is_irq: bool,
        is_no_alloc: bool,
        is_lock_free: bool,
        file: &str,
        line: usize,
        bag: &mut ErrorBag,
    ) {
        match expr {
            Expr::Call(name, args) => {
                for a in args {
                    walk_expr(
                        program,
                        a,
                        caller,
                        is_irq,
                        is_no_alloc,
                        is_lock_free,
                        file,
                        line,
                        bag,
                    );
                }
                let callee = program.functions.iter().find(|f| f.name == *name);
                let callee_may_sleep = matches!(name.as_str(), "cpu_halt" | "cpu_sti")
                    || callee
                        .map(|f| f.attrs.iter().any(|a| a == "may_sleep"))
                        .unwrap_or(false);
                let callee_locks = name.starts_with("acquire_")
                    || name.starts_with("lock_")
                    || callee
                        .map(|f| f.attrs.iter().any(|a| a == "locks"))
                        .unwrap_or(false);
                let callee_allocs = name.starts_with("alloc_")
                    || name == "malloc"
                    || callee
                        .map(|f| f.attrs.iter().any(|a| a == "allocates"))
                        .unwrap_or(false);

                if is_irq && callee_may_sleep {
                    bag.push(
                        Stage::Verifier,
                        file,
                        line,
                        format!(
                            "Context #[irq_safe]/#[interrupt] in '{}' forbids blocking call '{}'!",
                            caller.name, name
                        ),
                    );
                }
                if is_lock_free && callee_locks {
                    bag.push(
                        Stage::Verifier,
                        file,
                        line,
                        format!("Context #[lock_free]/#[interrupt] in '{}' forbids acquiring lock '{}' (deadlock prevention)!", caller.name, name),
                    );
                }
                if is_no_alloc && callee_allocs {
                    bag.push(
                        Stage::Verifier,
                        file,
                        line,
                        format!(
                            "Context #[no_alloc] in '{}' forbids memory allocation '{}'!",
                            caller.name, name
                        ),
                    );
                }
            }
            Expr::MethodCall(_, m, args) => {
                for a in args {
                    walk_expr(
                        program,
                        a,
                        caller,
                        is_irq,
                        is_no_alloc,
                        is_lock_free,
                        file,
                        line,
                        bag,
                    );
                }
                if is_lock_free && (m.starts_with("lock") || m.starts_with("acquire")) {
                    bag.push(
                        Stage::Verifier,
                        file,
                        line,
                        format!(
                            "Context #[lock_free] in '{}' forbids locking method '{}'!",
                            caller.name, m
                        ),
                    );
                }
            }
            Expr::BinOp(l, _, r) => {
                walk_expr(
                    program,
                    l,
                    caller,
                    is_irq,
                    is_no_alloc,
                    is_lock_free,
                    file,
                    line,
                    bag,
                );
                walk_expr(
                    program,
                    r,
                    caller,
                    is_irq,
                    is_no_alloc,
                    is_lock_free,
                    file,
                    line,
                    bag,
                );
            }
            Expr::UnaryNot(i) => walk_expr(
                program,
                i,
                caller,
                is_irq,
                is_no_alloc,
                is_lock_free,
                file,
                line,
                bag,
            ),
            _ => {}
        }
    }

    for s in stmts {
        match s {
            Stmt::Let { line, value, .. }
            | Stmt::Assign { line, value, .. }
            | Stmt::FieldAssign { line, value, .. }
            | Stmt::IndexAssign { line, value, .. }
            | Stmt::ExprStmt { line, expr: value }
            | Stmt::Return { line, expr: value } => {
                walk_expr(
                    program,
                    value,
                    func,
                    is_irq,
                    is_no_alloc,
                    is_lock_free,
                    file,
                    *line,
                    bag,
                );
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                walk_expr(
                    program,
                    cond,
                    func,
                    is_irq,
                    is_no_alloc,
                    is_lock_free,
                    file,
                    0,
                    bag,
                );
                check_execution_contexts(program, then_body, func, file, bag);
                check_execution_contexts(program, else_body, func, file, bag);
            }
            Stmt::While { cond, body, .. } => {
                walk_expr(
                    program,
                    cond,
                    func,
                    is_irq,
                    is_no_alloc,
                    is_lock_free,
                    file,
                    0,
                    bag,
                );
                check_execution_contexts(program, body, func, file, bag);
            }
            Stmt::For { body, .. } | Stmt::Loop { body, .. } => {
                check_execution_contexts(program, body, func, file, bag);
            }
            Stmt::Match { expr, arms, .. } => {
                walk_expr(
                    program,
                    expr,
                    func,
                    is_irq,
                    is_no_alloc,
                    is_lock_free,
                    file,
                    0,
                    bag,
                );
                for a in arms {
                    check_execution_contexts(program, &a.body, func, file, bag);
                }
            }
            _ => {}
        }
    }
}

fn check_stack_safety(program: &Program, is_kernel: bool, file: &str, bag: &mut ErrorBag) {
    fn stmt_frame_bytes(stmts: &[Stmt]) -> usize {
        let mut bytes = 16;
        for s in stmts {
            match s {
                Stmt::Let { .. } | Stmt::For { .. } => bytes += 4,
                Stmt::LetArray { len, .. } => bytes += len * 4,
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    bytes += stmt_frame_bytes(then_body).max(stmt_frame_bytes(else_body));
                }
                Stmt::While { body, .. } | Stmt::Loop { body, .. } => {
                    bytes += stmt_frame_bytes(body);
                }
                Stmt::Match { arms, .. } => {
                    let max_arm = arms
                        .iter()
                        .map(|a| stmt_frame_bytes(&a.body))
                        .max()
                        .unwrap_or(0);
                    bytes += max_arm;
                }
                _ => {}
            }
        }
        (bytes + 15) & !15
    }

    fn collect_calls(stmts: &[Stmt], out: &mut Vec<String>) {
        fn expr_calls(e: &Expr, out: &mut Vec<String>) {
            match e {
                Expr::Call(n, args) => {
                    out.push(n.clone());
                    for a in args {
                        expr_calls(a, out);
                    }
                }
                Expr::MethodCall(_, m, args) => {
                    out.push(format!("__{}", m));
                    for a in args {
                        expr_calls(a, out);
                    }
                }
                Expr::BinOp(l, _, r) => {
                    expr_calls(l, out);
                    expr_calls(r, out);
                }
                Expr::UnaryNot(i) => expr_calls(i, out),
                _ => {}
            }
        }
        for s in stmts {
            match s {
                Stmt::Let { value, .. }
                | Stmt::Assign { value, .. }
                | Stmt::FieldAssign { value, .. }
                | Stmt::IndexAssign { value, .. }
                | Stmt::ExprStmt { expr: value, .. }
                | Stmt::Return { expr: value, .. } => expr_calls(value, out),
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                    ..
                } => {
                    expr_calls(cond, out);
                    collect_calls(then_body, out);
                    collect_calls(else_body, out);
                }
                Stmt::While { cond, body, .. } => {
                    expr_calls(cond, out);
                    collect_calls(body, out);
                }
                Stmt::For { body, .. } | Stmt::Loop { body, .. } => collect_calls(body, out),
                Stmt::Match { expr, arms, .. } => {
                    expr_calls(expr, out);
                    for a in arms {
                        collect_calls(&a.body, out);
                    }
                }
                _ => {}
            }
        }
    }

    let mut frame_map: HashMap<String, usize> = HashMap::new();
    let mut call_map: HashMap<String, Vec<String>> = HashMap::new();
    for f in &program.functions {
        let mut fbytes = stmt_frame_bytes(&f.body) + f.params.len() * 4;
        if f.attrs.iter().any(|a| a == "interrupt") {
            fbytes += 32;
        }
        frame_map.insert(f.name.clone(), fbytes);
        let mut calls = Vec::new();
        collect_calls(&f.body, &mut calls);
        call_map.insert(f.name.clone(), calls);
    }

    fn dfs_stack(
        fn_name: &str,
        program: &Program,
        frame_map: &HashMap<String, usize>,
        call_map: &HashMap<String, Vec<String>>,
        visiting: &mut Vec<String>,
        file: &str,
        bag: &mut ErrorBag,
    ) -> usize {
        let fdef = program.functions.iter().find(|f| f.name == fn_name);
        let line = fdef.map(|f| f.line).unwrap_or(1);
        let max_depth: Option<usize> = fdef.and_then(|f| {
            f.attrs.iter().find_map(|a| {
                a.strip_prefix("max_depth:")
                    .and_then(|r| r.split("..").next())
                    .and_then(|v| v.parse::<usize>().ok())
            })
        });

        let base = *frame_map.get(fn_name).unwrap_or(&16);
        if visiting.contains(&fn_name.to_string()) {
            if let Some(d) = max_depth {
                return base * d;
            }
            bag.push(
                Stage::Verifier,
                file,
                line,
                format!(
                    "Unbounded recursion in '{}' risks stack overflow (annotate with #[max_depth(N)])!",
                    fn_name
                ),
            );
            return base;
        }
        visiting.push(fn_name.to_string());
        let mut max_child = 0usize;
        if let Some(callees) = call_map.get(fn_name) {
            for c in callees {
                let target = if c.starts_with("__") {
                    program
                        .functions
                        .iter()
                        .find(|f| f.name.ends_with(c))
                        .map(|f| f.name.clone())
                        .unwrap_or_default()
                } else {
                    c.clone()
                };
                if frame_map.contains_key(&target) {
                    max_child = max_child.max(dfs_stack(
                        &target, program, frame_map, call_map, visiting, file, bag,
                    ));
                }
            }
        }
        visiting.pop();
        base + max_child
    }

    let mut total_stack = 0usize;
    if frame_map.contains_key("main") {
        let mut visiting = Vec::new();
        total_stack += dfs_stack(
            "main",
            program,
            &frame_map,
            &call_map,
            &mut visiting,
            file,
            bag,
        );
    }
    for f in &program.functions {
        if f.attrs.iter().any(|a| a == "interrupt") {
            let mut visiting = Vec::new();
            total_stack += dfs_stack(
                &f.name,
                program,
                &frame_map,
                &call_map,
                &mut visiting,
                file,
                bag,
            );
        }
    }

    let limit = program
        .functions
        .iter()
        .find(|f| f.name == "main")
        .and_then(|f| {
            f.attrs.iter().find_map(|a| {
                a.strip_prefix("stack_limit:")
                    .and_then(|r| r.split("..").next())
                    .and_then(|v| v.parse::<usize>().ok())
            })
        })
        .unwrap_or(if is_kernel { 16384 } else { 1048576 });

    if total_stack > limit {
        bag.push(
            Stage::Verifier,
            file,
            1,
            format!(
                "Zero-Stack-Overflow: static program stack ({} bytes) exceeds limit ({} bytes)!",
                total_stack, limit
            ),
        );
    }
}

fn check_typestate(
    program: &Program,
    stmts: &[Stmt],
    func: &Function,
    states: &mut std::collections::HashSet<String>,
    file: &str,
    bag: &mut ErrorBag,
) {
    fn eval_call_states(
        program: &Program,
        expr: &Expr,
        caller: &Function,
        states: &mut std::collections::HashSet<String>,
        file: &str,
        line: usize,
        bag: &mut ErrorBag,
    ) {
        match expr {
            Expr::Call(name, args) => {
                for a in args {
                    eval_call_states(program, a, caller, states, file, line, bag);
                }
                if name == "idt_init" {
                    states.insert("IdtReady".to_string());
                } else if name == "cpu_cli" {
                    states.insert("IrqDisabled".to_string());
                } else if name == "cpu_sti" {
                    if !states.contains("IdtReady") {
                        bag.push(
                            Stage::Verifier,
                            file,
                            line,
                            format!(
                                "Typestate error in '{}': 'cpu_sti()' is forbidden before IDT initialization (requires 'IdtReady')!",
                                caller.name
                            ),
                        );
                    }
                    states.remove("IrqDisabled");
                }
                if let Some(callee) = program.functions.iter().find(|f| f.name == *name) {
                    for attr in &callee.attrs {
                        if let Some(req_list) = attr.strip_prefix("requires:") {
                            for req in req_list.split(',').filter(|s| !s.is_empty()) {
                                if !states.contains(req) {
                                    bag.push(
                                        Stage::Verifier,
                                        file,
                                        line,
                                        format!(
                                            "Typestate error in '{}': call to '{}' requires state '{}', which is not proven here!",
                                            caller.name, callee.name, req
                                        ),
                                    );
                                }
                            }
                        }
                    }
                    for attr in &callee.attrs {
                        if let Some(ens_list) = attr.strip_prefix("ensures:") {
                            for ens in ens_list.split(',').filter(|s| !s.is_empty()) {
                                states.insert(ens.to_string());
                            }
                        }
                    }
                }
            }
            Expr::MethodCall(_, method, args) => {
                for a in args {
                    eval_call_states(program, a, caller, states, file, line, bag);
                }
                if let Some(callee) = program
                    .functions
                    .iter()
                    .find(|f| f.name.ends_with(&format!("__{}", method)))
                {
                    for attr in &callee.attrs {
                        if let Some(req_list) = attr.strip_prefix("requires:") {
                            for req in req_list.split(',').filter(|s| !s.is_empty()) {
                                if !states.contains(req) {
                                    bag.push(
                                        Stage::Verifier,
                                        file,
                                        line,
                                        format!(
                                            "Typestate error in '{}': method '{}' requires state '{}'!",
                                            caller.name, callee.name, req
                                        ),
                                    );
                                }
                            }
                        }
                    }
                    for attr in &callee.attrs {
                        if let Some(ens_list) = attr.strip_prefix("ensures:") {
                            for ens in ens_list.split(',').filter(|s| !s.is_empty()) {
                                states.insert(ens.to_string());
                            }
                        }
                    }
                }
            }
            Expr::BinOp(l, _, r) => {
                eval_call_states(program, l, caller, states, file, line, bag);
                eval_call_states(program, r, caller, states, file, line, bag);
            }
            Expr::UnaryNot(inner) => {
                eval_call_states(program, inner, caller, states, file, line, bag)
            }
            _ => {}
        }
    }

    for stmt in stmts {
        match stmt {
            Stmt::Let { line, value, .. }
            | Stmt::Assign { line, value, .. }
            | Stmt::FieldAssign { line, value, .. }
            | Stmt::IndexAssign { line, value, .. }
            | Stmt::ExprStmt { line, expr: value }
            | Stmt::Return { line, expr: value } => {
                eval_call_states(program, value, func, states, file, *line, bag);
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                eval_call_states(program, cond, func, states, file, 0, bag);
                let mut then_st = states.clone();
                let mut else_st = states.clone();
                check_typestate(program, then_body, func, &mut then_st, file, bag);
                check_typestate(program, else_body, func, &mut else_st, file, bag);

                *states = then_st.intersection(&else_st).cloned().collect();
            }
            Stmt::For { body, .. } | Stmt::While { body, .. } | Stmt::Loop { body, .. } => {
                let mut loop_st = states.clone();
                check_typestate(program, body, func, &mut loop_st, file, bag);
            }
            Stmt::Match { arms, .. } => {
                for arm in arms {
                    let mut arm_st = states.clone();
                    check_typestate(program, &arm.body, func, &mut arm_st, file, bag);
                }
            }
            _ => {}
        }
    }
}

fn check_linear_resources(
    stmts: &[Stmt],
    func: &Function,
    linear_structs: &std::collections::HashSet<String>,
    live: &mut std::collections::HashMap<String, usize>,
    file: &str,
    bag: &mut ErrorBag,
) {
    fn consume_expr(
        expr: &Expr,
        live: &mut std::collections::HashMap<String, usize>,
        func: &Function,
        file: &str,
        line: usize,
        bag: &mut ErrorBag,
    ) {
        match expr {
            Expr::Call(_, args) | Expr::MethodCall(_, _, args) => {
                for a in args {
                    if let Expr::Var(v) = a {
                        if live.remove(v).is_none() {}
                    } else {
                        consume_expr(a, live, func, file, line, bag);
                    }
                }
            }
            Expr::BinOp(l, _, r) => {
                consume_expr(l, live, func, file, line, bag);
                consume_expr(r, live, func, file, line, bag);
            }
            Expr::UnaryNot(inner) => consume_expr(inner, live, func, file, line, bag),
            _ => {}
        }
    }

    for stmt in stmts {
        match stmt {
            Stmt::Let {
                line,
                name,
                type_name,
                value,
                ..
            } => {
                consume_expr(value, live, func, file, *line, bag);
                if linear_structs.contains(type_name) {
                    live.insert(name.clone(), *line);
                }
            }
            Stmt::ExprStmt { line, expr } => {
                consume_expr(expr, live, func, file, *line, bag);
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                consume_expr(cond, live, func, file, 0, bag);
                let mut then_live = live.clone();
                let mut else_live = live.clone();
                check_linear_resources(then_body, func, linear_structs, &mut then_live, file, bag);
                check_linear_resources(else_body, func, linear_structs, &mut else_live, file, bag);

                live.clear();
                live.extend(then_live);
                live.extend(else_live);
            }
            Stmt::Match { expr, arms, .. } => {
                consume_expr(expr, live, func, file, 0, bag);
                let base_live = live.clone();
                live.clear();
                for arm in arms {
                    let mut arm_live = base_live.clone();
                    check_linear_resources(
                        &arm.body,
                        func,
                        linear_structs,
                        &mut arm_live,
                        file,
                        bag,
                    );
                    live.extend(arm_live);
                }
            }
            Stmt::For { body, .. } | Stmt::While { body, .. } | Stmt::Loop { body, .. } => {
                check_linear_resources(body, func, linear_structs, live, file, bag);
            }
            Stmt::Return { line, expr } => {
                if let Expr::Var(v) = expr {
                    live.remove(v);
                } else {
                    consume_expr(expr, live, func, file, *line, bag);
                }
                for (res_name, decl_line) in live.iter() {
                    bag.push(
                        Stage::Verifier,
                        file,
                        *line,
                        format!(
                            "Linear resource leak of '{}' (declared at line {}) in '{}' — not consumed before return!",
                            res_name, decl_line, func.name
                        ),
                    );
                }
                live.clear();
            }
            _ => {}
        }
    }
}

fn check_termination(
    stmts: &[Stmt],
    func: &Function,
    env: &HashMap<String, (i128, i128)>,
    arrays: &HashMap<String, ArrayInfo>,
    sigs: &HashMap<String, FnSig>,
    file: &str,
    bag: &mut ErrorBag,
) {
    let has_term = func
        .attrs
        .iter()
        .any(|a| a == "terminates" || a == "interrupt" || a.starts_with("max_cycles:"));
    if !has_term {
        return;
    }
    let max_cycles: Option<i128> = func.attrs.iter().find_map(|a| {
        a.strip_prefix("max_cycles:")
            .and_then(|r| r.split("..").next())
            .and_then(|v| v.parse::<i128>().ok())
    });

    for stmt in stmts {
        match stmt {
            Stmt::Loop { line, .. } => {
                bag.push(
                    Stage::Verifier,
                    file,
                    *line,
                    format!(
                        "Function '{}' is marked #[terminates]/#[interrupt]; infinite 'loop' is forbidden!",
                        func.name
                    ),
                );
            }
            Stmt::For {
                line,
                start,
                end,
                body,
                ..
            } => {
                if let (Ok((s_lo, _)), Ok((_, e_hi)), Some(limit)) = (
                    eval_expr_range(start, env, arrays, sigs),
                    eval_expr_range(end, env, arrays, sigs),
                    max_cycles,
                ) {
                    let iters = (e_hi - s_lo).max(0);
                    if iters > limit {
                        bag.push(
                            Stage::Verifier,
                            file,
                            *line,
                            format!(
                                "For loop in '{}' requires up to {} iterations, exceeding #[max_cycles({})] limit!",
                                func.name, iters, limit
                            ),
                        );
                    }
                }
                check_termination(body, func, env, arrays, sigs, file, bag);
            }
            Stmt::While { line, cond, body } => {
                let prog_var = match cond {
                    Expr::BinOp(l, op, _)
                        if matches!(op.as_str(), "<" | "<=" | ">" | ">=" | "!=") =>
                    {
                        if let Expr::Var(v) = l.as_ref() {
                            Some((v.clone(), op.clone()))
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                let mut has_progress = false;
                if let Some((ref vname, ref op)) = prog_var {
                    for s in body {
                        if let Stmt::Assign { name, value, .. } = s {
                            if name == vname {
                                if let Expr::BinOp(bl, bop, br) = value {
                                    if let (Expr::Var(bv), Expr::Number(step)) =
                                        (bl.as_ref(), br.as_ref())
                                    {
                                        if bv == vname {
                                            if (matches!(op.as_str(), "<" | "<=")
                                                && bop == "+"
                                                && *step > 0)
                                                || (matches!(op.as_str(), ">" | ">=")
                                                    && bop == "-"
                                                    && *step > 0)
                                            {
                                                has_progress = true;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if !has_progress {
                    bag.push(
                        Stage::Verifier,
                        file,
                        *line,
                        format!(
                            "While loop in '{}' (#[terminates]) has no proven termination variant!",
                            func.name
                        ),
                    );
                }
                check_termination(body, func, env, arrays, sigs, file, bag);
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                check_termination(then_body, func, env, arrays, sigs, file, bag);
                check_termination(else_body, func, env, arrays, sigs, file, bag);
            }
            Stmt::Match { arms, .. } => {
                for arm in arms {
                    check_termination(&arm.body, func, env, arrays, sigs, file, bag);
                }
            }
            _ => {}
        }
    }
}

fn check_hw_contracts(
    program: &Program,
    expr: &Expr,
    func: &Function,
    env: &HashMap<String, (i128, i128)>,
    arrays: &HashMap<String, ArrayInfo>,
    sigs: &HashMap<String, FnSig>,
    file: &str,
    line: usize,
    bag: &mut ErrorBag,
) {
    if let Expr::Call(name, args) = expr {
        if let Some(callee) = program.functions.iter().find(|f| f.name == *name) {
            for attr in &callee.attrs {
                for (prefix, tag) in [("mmio:", "#[mmio"), ("port:", "#[port")] {
                    if let Some(rest) = attr.strip_prefix(prefix) {
                        if let Some((lo_s, hi_s)) = rest.split_once("..") {
                            if let (Ok(lo), Ok(hi)) = (lo_s.parse::<i128>(), hi_s.parse::<i128>()) {
                                let has_perm = func.attrs.iter().any(|fa| {
                                    fa.strip_prefix(prefix)
                                        .and_then(|r| r.split_once(".."))
                                        .and_then(|(a, b)| {
                                            Some((a.parse::<i128>().ok()?, b.parse::<i128>().ok()?))
                                        })
                                        .map(|(flo, fhi)| flo <= lo && fhi >= hi)
                                        .unwrap_or(false)
                                });
                                if !has_perm {
                                    bag.push(
                                        Stage::Verifier,
                                        file,
                                        line,
                                        format!("Function '{}' calls '{}', which requires capability {}(0x{:X}..0x{:X})]!", func.name, callee.name, tag, lo, hi),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        let is_mem = matches!(
            name.as_str(),
            "mem_read8" | "mem_write8" | "mem_read32" | "mem_write32"
        );
        let is_port = matches!(name.as_str(), "port_in8" | "port_out8");
        if (is_mem || is_port) && !args.is_empty() {
            let prefix = if is_mem { "mmio:" } else { "port:" };
            let kind_label = if is_mem {
                "physical memory #[mmio(lo..hi)]"
            } else {
                "I/O ports #[port(lo..hi)]"
            };
            let mut allowed: Vec<(i128, i128)> = Vec::new();
            if is_port && func.attrs.iter().any(|a| a == "interrupt") {
                allowed.push((0x20, 0x21));
                allowed.push((0xA0, 0xA1));
            }
            for a in &func.attrs {
                if let Some(rest) = a.strip_prefix(prefix) {
                    if let Some((lo_s, hi_s)) = rest.split_once("..") {
                        if let (Ok(lo), Ok(hi)) = (lo_s.parse::<i128>(), hi_s.parse::<i128>()) {
                            allowed.push((lo, hi));
                        }
                    }
                }
            }
            if allowed.is_empty() {
                bag.push(
                    Stage::Verifier,
                    file,
                    line,
                    format!(
                        "Call to '{}' is forbidden without {} capability on function '{}'!",
                        name, kind_label, func.name
                    ),
                );
            } else if let Ok((amin, amax)) = eval_expr_range(&args[0], env, arrays, sigs) {
                let ok = allowed.iter().any(|(lo, hi)| amin >= *lo && amax <= *hi);
                if !ok {
                    bag.push(
                        Stage::Verifier,
                        file,
                        line,
                        format!(
                            "Capability violation for {}: '{}' accesses [0x{:X}..0x{:X}], outside permitted bounds!",
                            kind_label, name, amin, amax
                        ),
                    );
                }
            }
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn emit_stmts(
    program: &Program,
    stmts: &[Stmt],
    func: &Function,
    env: &mut HashMap<String, (i128, i128)>,
    declared: &mut HashMap<String, (i128, i128)>,
    arrays: &mut HashMap<String, ArrayInfo>,
    sigs: &HashMap<String, FnSig>,
    file: &str,
    source: &str,
    indent: &str,
    out: &mut String,
    bag: &mut ErrorBag,
) {
    let _ = source;
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                line,
                name,
                type_name,
                min_val,
                max_val,
                value,
            } => {
                check_hw_contracts(program, value, func, env, arrays, sigs, file, *line, bag);
                if let Expr::StructInit(st_name, inits) = value {
                    for (fname, fexpr) in inits {
                        let key = format!("{}.{}", name, fname);
                        let (fmin, fmax) = eval_expr_range(fexpr, env, arrays, sigs)
                            .unwrap_or((i32::MIN as i128, i32::MAX as i128));
                        if let Some(&(cmin, cmax)) =
                            declared.get(&format!("__struct_{}.{}", st_name, fname))
                        {
                            if fmin < cmin || fmax > cmax {
                                bag.push(
                                    Stage::Verifier,
                                    file,
                                    *line,
                                    format!(
                                        "Field {}.{} violates contract <{}..{}> (evaluates to [{}..{}])!",
                                        st_name, fname, cmin, cmax, fmin, fmax
                                    ),
                                );
                            }
                            env.insert(key.clone(), (fmin, fmax));
                            declared.insert(key, (cmin, cmax));
                        } else {
                            env.insert(key.clone(), (fmin, fmax));
                            declared.insert(key, (i32::MIN as i128, i32::MAX as i128));
                        }
                    }
                    env.insert(name.clone(), (0, 0));
                    out.push_str(&format!(
                        "{}struct {} {} = {};\n",
                        indent,
                        type_name,
                        name,
                        expr_to_c(value)
                    ));
                    continue;
                }
                if *min_val > *max_val {
                    bag.push(
                        Stage::Verifier,
                        file,
                        *line,
                        format!(
                            "Invalid range for {}: min ({}) > max ({})!",
                            name, min_val, max_val
                        ),
                    );
                }
                match eval_expr_range(value, env, arrays, sigs) {
                    Ok((expr_min, expr_max)) => {
                        if expr_min < *min_val as i128 || expr_max > *max_val as i128 {
                            bag.push(Stage::Verifier, file, *line, format!("Range error: {} is declared as <{}..{}>, but expression evaluates to [{}..{}]!", name, min_val, max_val, expr_min, expr_max));
                        }
                    }
                    Err(err) => {
                        if *min_val != i64::MIN || *max_val != i64::MAX {
                            bag.push(
                                Stage::Verifier,
                                file,
                                *line,
                                format!(
                                    "Cannot prove range for {} (<{}..{}>): {}",
                                    name, min_val, max_val, err
                                ),
                            );
                        }
                    }
                }
                if program
                    .enums
                    .iter()
                    .any(|e| e.name == *type_name && e.variants.iter().any(|(_, _, p)| p.is_some()))
                {
                    if let Err(err) = eval_expr_range(value, declared, arrays, sigs) {
                        bag.push(Stage::Verifier, file, *line, err);
                    }
                    env.insert(name.clone(), (0, 0));
                    out.push_str(&format!(
                        "{}struct {} {} = {};\n",
                        indent,
                        type_name,
                        name,
                        expr_to_c(value)
                    ));
                    continue;
                }
                if let Some(&ebounds) = declared.get(&format!("_enum{}", type_name)) {
                    env.insert(name.clone(), ebounds);
                    declared.insert(name.clone(), ebounds);
                } else if !(*min_val == i64::MIN && *max_val == i64::MAX) {
                    let bounds = (*min_val as i128, *max_val as i128);
                    env.insert(name.clone(), bounds);
                    declared.insert(name.clone(), bounds);
                }
                if let Expr::Call(fn_name, _) = value {
                    if let Some(sig) = sigs.get(fn_name) {
                        if sig.ret_min != i64::MIN || sig.ret_max != i64::MAX {
                            env.insert(name.clone(), (sig.ret_min as i128, sig.ret_max as i128));
                        }
                    }
                }
                if !env.contains_key(name) {
                    env.insert(name.clone(), (i32::MIN as i128, i32::MAX as i128));
                }
                out.push_str(&format!(
                    "{}int32_t {} = {};\n",
                    indent,
                    name,
                    expr_to_c(value)
                ));
            }
            Stmt::LetArray {
                line,
                name,
                min_val,
                max_val,
                len,
                elements,
            } => {
                if elements.len() != *len {
                    bag.push(
                        Stage::Verifier,
                        file,
                        *line,
                        format!(
                            "Array {} declared with length {}, but {} elements provided!",
                            name,
                            len,
                            elements.len()
                        ),
                    );
                }
                let mut elems_c = Vec::new();
                for (i, el) in elements.iter().enumerate() {
                    match eval_expr_range(el, env, arrays, sigs) {
                        Ok((emin, emax)) => {
                            if emin < *min_val as i128 || emax > *max_val as i128 {
                                bag.push(
                                    Stage::Verifier,
                                    file,
                                    *line,
                                    format!(
                                        "Element {}[{}] exceeds range <{}..{}>!",
                                        name, i, min_val, max_val
                                    ),
                                );
                            }
                        }
                        Err(err) => bag.push(
                            Stage::Verifier,
                            file,
                            *line,
                            format!("Error in element {}[{}]: {}", name, i, err),
                        ),
                    }
                    elems_c.push(expr_to_c(el));
                }
                env.insert(name.clone(), (0, 0));
                arrays.insert(
                    name.clone(),
                    ArrayInfo {
                        len: *len,
                        elem_min: *min_val as i128,
                        elem_max: *max_val as i128,
                    },
                );
                out.push_str(&format!(
                    "{}int32_t {}[{}] = {{{}}};\n",
                    indent,
                    name,
                    len,
                    elems_c.join(", ")
                ));
            }
            Stmt::FieldAssign {
                line,
                obj,
                field,
                value,
            } => {
                let key = format!("{}.{}", obj, field);
                if let Some(&(decl_min, decl_max)) = declared.get(&key) {
                    match eval_expr_range(value, env, arrays, sigs) {
                        Ok((emin, emax)) => {
                            if emin < decl_min || emax > decl_max {
                                bag.push(
                                    Stage::Verifier,
                                    file,
                                    *line,
                                    format!(
                                        "Field {} violates contract <{}..{}> (evaluates to [{}..{}])!",
                                        key, decl_min, decl_max, emin, emax
                                    ),
                                );
                            }
                            env.insert(key, (emin, emax));
                        }
                        Err(err) => bag.push(
                            Stage::Verifier,
                            file,
                            *line,
                            format!("Cannot prove field {}: {}", key, err),
                        ),
                    }
                }
                out.push_str(&format!(
                    "{}{}.{} = {};\n",
                    indent,
                    obj,
                    field,
                    expr_to_c(value)
                ));
            }
            Stmt::Assign { line, name, value } => {
                if let Some(&(decl_min, decl_max)) = declared.get(name) {
                    match eval_expr_range(value, env, arrays, sigs) {
                        Ok((expr_min, expr_max)) => {
                            if expr_min < decl_min || expr_max > decl_max {
                                bag.push(Stage::Verifier, file, *line, format!("Assignment to {} violates contract <{}..{}> (evaluates to [{}..{}])!", name, decl_min, decl_max, expr_min, expr_max));
                            }
                            env.insert(name.clone(), (expr_min, expr_max));
                        }
                        Err(err) => bag.push(
                            Stage::Verifier,
                            file,
                            *line,
                            format!("Cannot prove assignment to {}: {}", name, err),
                        ),
                    }
                }
                out.push_str(&format!("{}{} = {};\n", indent, name, expr_to_c(value)));
            }
            Stmt::IndexAssign {
                line,
                name,
                index,
                value,
            } => {
                if let Some(info) = arrays.get(name) {
                    match eval_expr_range(index, env, arrays, sigs) {
                        Ok((imin, imax)) => {
                            if imin < 0 || imax >= info.len as i128 {
                                bag.push(
                                    Stage::Verifier,
                                    file,
                                    *line,
                                    format!(
                                        "Out-of-bounds write to {}[0..{}]: [{}..{}]!",
                                        name,
                                        info.len - 1,
                                        imin,
                                        imax
                                    ),
                                );
                            }
                        }
                        Err(err) => bag.push(
                            Stage::Verifier,
                            file,
                            *line,
                            format!("Cannot prove write index for {}: {}", name, err),
                        ),
                    }
                    match eval_expr_range(value, env, arrays, sigs) {
                        Ok((vmin, vmax)) => {
                            if vmin < info.elem_min || vmax > info.elem_max {
                                bag.push(
                                    Stage::Verifier,
                                    file,
                                    *line,
                                    format!(
                                        "Value [{}..{}] exceeds element bounds of array {}!",
                                        vmin, vmax, name
                                    ),
                                );
                            }
                        }
                        Err(err) => bag.push(
                            Stage::Verifier,
                            file,
                            *line,
                            format!("Cannot prove value for {}: {}", name, err),
                        ),
                    }
                }
                out.push_str(&format!(
                    "{}{}[{}] = {};\n",
                    indent,
                    name,
                    expr_to_c(index),
                    expr_to_c(value)
                ));
            }
            Stmt::Loop { body, .. } => {
                for (k, v) in declared.iter() {
                    env.insert(k.clone(), *v);
                }
                out.push_str(&format!("{}while (1) {{\n", indent));
                emit_stmts(
                    program,
                    body,
                    func,
                    env,
                    declared,
                    arrays,
                    sigs,
                    file,
                    source,
                    &format!("{}    ", indent),
                    out,
                    bag,
                );
                out.push_str(&format!("{}}}\n", indent));
            }
            Stmt::For {
                line,
                var,
                start,
                end,
                body,
            } => {
                let (s_lo, _) = eval_expr_range(start, env, arrays, sigs).unwrap_or_else(|e| {
                    bag.push(
                        Stage::Verifier,
                        file,
                        *line,
                        format!("Invalid for-loop start bound: {}", e),
                    );
                    (0, 0)
                });
                let (_e_lo, e_hi) = eval_expr_range(end, env, arrays, sigs).unwrap_or_else(|e| {
                    bag.push(
                        Stage::Verifier,
                        file,
                        *line,
                        format!("Invalid for-loop end bound: {}", e),
                    );
                    (0, 0)
                });
                if s_lo > e_hi {
                    bag.push(
                        Stage::Verifier,
                        file,
                        *line,
                        format!("Invalid for-loop range ({}..{})!", s_lo, e_hi),
                    );
                }
                let mut loop_env = env.clone();
                let mut loop_decl = declared.clone();
                let iter_hi = (e_hi - 1).max(s_lo);
                loop_env.insert(var.clone(), (s_lo, iter_hi));
                loop_decl.insert(var.clone(), (s_lo, iter_hi));
                out.push_str(&format!(
                    "{}for (int32_t {} = {}; {} < {}; {}++) {{\n",
                    indent,
                    var,
                    expr_to_c(start),
                    var,
                    expr_to_c(end),
                    var
                ));
                emit_stmts(
                    program,
                    body,
                    func,
                    &mut loop_env,
                    &mut loop_decl,
                    arrays,
                    sigs,
                    file,
                    source,
                    &format!("{}    ", indent),
                    out,
                    bag,
                );
                out.push_str(&format!("{}}}\n", indent));
            }
            Stmt::While { cond, body, .. } => {
                out.push_str(&format!("{}while ({}) {{\n", indent, expr_to_c(cond)));
                let (mut body_env, exit_env) = refine_envs(cond, env);
                let mut probe_env = body_env.clone();
                let mut probe_decl = declared.clone();
                let mut probe_out = String::new();
                let mut probe_bag = ErrorBag::default();
                emit_stmts(
                    program,
                    body,
                    func,
                    &mut probe_env,
                    &mut probe_decl,
                    arrays,
                    sigs,
                    file,
                    source,
                    "",
                    &mut probe_out,
                    &mut probe_bag,
                );
                for (k, &(after_lo, after_hi)) in probe_env.iter() {
                    if let Some(&(before_lo, before_hi)) = body_env.get(k) {
                        if let Some(&(decl_lo, decl_hi)) = declared.get(k) {
                            let wide_lo = if after_lo < before_lo {
                                decl_lo
                            } else {
                                before_lo
                            };
                            let wide_hi = if after_hi > before_hi {
                                decl_hi
                            } else {
                                before_hi
                            };
                            body_env.insert(k.clone(), (wide_lo, wide_hi));
                        }
                    }
                }
                let (refined_body_env, _) = refine_envs(cond, &body_env);
                body_env = refined_body_env;
                emit_stmts(
                    program,
                    body,
                    func,
                    &mut body_env,
                    declared,
                    arrays,
                    sigs,
                    file,
                    source,
                    &format!("{}    ", indent),
                    out,
                    bag,
                );
                out.push_str(&format!("{}}}\n", indent));
                for (k, v) in exit_env {
                    if !body.iter().any(|st| matches!(st, Stmt::Break { .. })) {
                        env.insert(k, v);
                    }
                }
            }
            Stmt::Break { .. } => out.push_str(&format!("{}break;\n", indent)),
            Stmt::Continue { .. } => out.push_str(&format!("{}continue;\n", indent)),
            Stmt::ExprStmt { line, expr } => {
                check_hw_contracts(program, expr, func, env, arrays, sigs, file, *line, bag);
                if let Err(err) = eval_expr_range(expr, env, arrays, sigs) {
                    bag.push(
                        Stage::Verifier,
                        file,
                        *line,
                        format!("Call verification error: {}", err),
                    );
                }
                out.push_str(&format!("{}{};\n", indent, expr_to_c(expr)));
            }
            Stmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                out.push_str(&format!("{}if ({}) {{\n", indent, expr_to_c(cond)));
                let (mut then_env, mut else_env) = refine_envs(cond, env);
                let mut then_decl = declared.clone();
                let mut else_decl = declared.clone();
                emit_stmts(
                    program,
                    then_body,
                    func,
                    &mut then_env,
                    &mut then_decl,
                    arrays,
                    sigs,
                    file,
                    source,
                    &format!("{}    ", indent),
                    out,
                    bag,
                );
                if !else_body.is_empty() {
                    out.push_str(&format!("{}}} else {{\n", indent));
                    emit_stmts(
                        program,
                        else_body,
                        func,
                        &mut else_env,
                        &mut else_decl,
                        arrays,
                        sigs,
                        file,
                        source,
                        &format!("{}    ", indent),
                        out,
                        bag,
                    );
                }
                out.push_str(&format!("{}}}\n", indent));
                if block_always_returns(then_body) {
                    *env = else_env;
                } else if !else_body.is_empty() && block_always_returns(else_body) {
                    *env = then_env;
                } else {
                    for (k, &(t_lo, t_hi)) in then_env.iter() {
                        if let Some(&(e_lo, e_hi)) = else_env.get(k) {
                            if let Some(&(orig_lo, orig_hi)) = env.get(k) {
                                let new_lo = t_lo.min(e_lo).max(orig_lo);
                                let new_hi = t_hi.max(e_hi).min(orig_hi);
                                if new_lo <= new_hi {
                                    env.insert(k.clone(), (new_lo, new_hi));
                                }
                            }
                        }
                    }
                }
            }
            Stmt::Match { line, expr, arms } => {
                let subj_c = expr_to_c(expr);
                let subj_var = match expr {
                    Expr::Var(v) => Some(v.clone()),
                    Expr::Field(o, f) => Some(format!("{}.{}", o, f)),
                    _ => None,
                };
                let (subj_lo, subj_hi) = eval_expr_range(expr, env, arrays, sigs)
                    .unwrap_or((i32::MIN as i128, i32::MAX as i128));
                let mut has_wildcard = false;
                let mut covered_enums: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                let mut enum_type_opt: Option<String> = None;
                let mut covered_intervals: Vec<(i128, i128)> = Vec::new();
                for (idx, arm) in arms.iter().enumerate() {
                    let kw = if idx == 0 { "if" } else { "} else if" };
                    let mut arm_env = env.clone();
                    let mut arm_decl = declared.clone();
                    match &arm.pat {
                        MatchPat::Wildcard => {
                            has_wildcard = true;
                            if idx == 0 {
                                out.push_str(&format!("{}{{\n", indent));
                            } else {
                                out.push_str(&format!("{}}} else {{\n", indent));
                            }
                        }
                        MatchPat::Num(n) => {
                            let c = *n as i128;
                            covered_intervals.push((c, c));
                            if let Some(ref v) = subj_var {
                                arm_env.insert(v.clone(), (c, c));
                            }
                            out.push_str(&format!("{}{} ({} == {}) {{\n", indent, kw, subj_c, n));
                        }
                        MatchPat::Range(lo, hi) => {
                            let l = *lo as i128;
                            let h = *hi as i128;
                            if l > h {
                                bag.push(
                                    Stage::Verifier,
                                    file,
                                    arm.line,
                                    format!("Invalid match range: {}..{}", lo, hi),
                                );
                            }
                            covered_intervals.push((l, h));
                            if let Some(ref v) = subj_var {
                                arm_env.insert(v.clone(), (l.max(subj_lo), h.min(subj_hi)));
                            }
                            out.push_str(&format!(
                                "{}{} ({} >= {} && {} <= {}) {{\n",
                                indent, kw, subj_c, lo, subj_c, hi
                            ));
                        }
                        MatchPat::EnumVal(en, var, bind) => {
                            enum_type_opt = Some(en.clone());
                            covered_enums.insert(var.clone());
                            let is_tagged = program.enums.iter().any(|e| {
                                e.name == *en && e.variants.iter().any(|(_, _, p)| p.is_some())
                            });
                            if let Some(&rng) = env.get(&format!("{}::{}", en, var)) {
                                covered_intervals.push(rng);
                                if let Some(ref v) = subj_var {
                                    arm_env.insert(v.clone(), rng);
                                }
                            }
                            if is_tagged {
                                out.push_str(&format!(
                                    "{}{} ({}.tag == {}{}) {{\n",
                                    indent, kw, subj_c, en, var
                                ));
                                if let Some(bname) = bind {
                                    let pbounds = env
                                        .get(&format!("__enum_payload_{}::{}", en, var))
                                        .copied()
                                        .unwrap_or((i32::MIN as i128, i32::MAX as i128));
                                    arm_env.insert(bname.clone(), pbounds);
                                    arm_decl.insert(bname.clone(), pbounds);
                                    out.push_str(&format!(
                                        "{}    int32_t {} = {}.payload;\n",
                                        indent, bname, subj_c
                                    ));
                                }
                            } else {
                                out.push_str(&format!(
                                    "{}{} ({} == {}{}) {{\n",
                                    indent, kw, subj_c, en, var
                                ));
                            }
                        }
                    }
                    emit_stmts(
                        program,
                        &arm.body,
                        func,
                        &mut arm_env,
                        &mut arm_decl,
                        arrays,
                        sigs,
                        file,
                        source,
                        &format!("{}    ", indent),
                        out,
                        bag,
                    );
                }
                if !arms.is_empty() {
                    out.push_str(&format!("{}}}\n", indent));
                }
                if !has_wildcard {
                    if let Some(en_name) = enum_type_opt {
                        if let Some(edef) = program.enums.iter().find(|e| e.name == en_name) {
                            let missing: Vec<String> = edef
                                .variants
                                .iter()
                                .filter(|(vn, _, _)| !covered_enums.contains(vn))
                                .map(|(vn, _, _)| format!("{}::{}", en_name, vn))
                                .collect();
                            if !missing.is_empty() {
                                bag.push(
                                    Stage::Verifier,
                                    file,
                                    *line,
                                    format!(
                                        "Non-exhaustive match on enum {}: missing variants [{}]",
                                        en_name,
                                        missing.join(", ")
                                    ),
                                );
                            }
                        }
                    } else if subj_lo <= subj_hi {
                        covered_intervals.sort_by_key(|r| r.0);
                        let mut cur = subj_lo;
                        for (lo, hi) in covered_intervals {
                            if hi < cur {
                                continue;
                            }
                            if lo > cur {
                                break;
                            }
                            cur = hi.saturating_add(1);
                            if cur > subj_hi {
                                break;
                            }
                        }
                        if cur <= subj_hi {
                            bag.push(
                                Stage::Verifier,
                                file,
                                *line,
                                format!(
"Non-exhaustive match: interval [{}..{}] is not covered (e.g. value {})",
subj_lo, subj_hi, cur
),
                            );
                        }
                    }
                }
            }
            Stmt::InlineAsm {
                line,
                template,
                clobbers,
            } => {
                let lower = template.to_lowercase();
                for reg in clobbers {
                    let r = reg.trim_start_matches('%').to_lowercase();
                    if r == "ebp" || r == "esp" || r == "rbp" || r == "rsp" {
                        bag.push(
                            Stage::Verifier,
                            file,
                            *line,
                            format!(
                                "Inline asm! in '{}' cannot clobber stack pointer '{}'!",
                                func.name, reg
                            ),
                        );
                    }
                }
                let uses_port = lower.contains("outb")
                    || lower.contains("outw")
                    || lower.contains("outl")
                    || lower.contains("inb")
                    || lower.contains("inw")
                    || lower.contains("inl");
                if uses_port && !func.attrs.iter().any(|a| a.starts_with("port:")) {
                    bag.push(
                        Stage::Verifier,
                        file,
                        *line,
                        format!("Inline asm! with I/O instructions in '{}' requires #[port(lo..hi)] contract!", func.name),
                    );
                }
                let is_irq = func
                    .attrs
                    .iter()
                    .any(|a| a == "interrupt" || a == "irq_safe");
                if is_irq && (lower.contains("hlt") || lower.contains("sti")) {
                    bag.push(
                        Stage::Verifier,
                        file,
                        *line,
                        format!("Inline asm! inside #[irq_safe]/#[interrupt] function '{}' forbids hlt/sti!", func.name),
                    );
                }
                if clobbers.is_empty() {
                    out.push_str(&format!("{}__asm__ volatile (\"{}\");\n", indent, template));
                } else {
                    let clob_str: Vec<String> = clobbers
                        .iter()
                        .map(|c| format!("\"{}\"", c.trim_start_matches('%')))
                        .collect();
                    out.push_str(&format!(
                        "{}__asm__ volatile (\"{}\" : : : {});\n",
                        indent,
                        template,
                        clob_str.join(", ")
                    ));
                }
            }
            Stmt::Prove { line, cond } => {
                #[cfg(feature = "inquisitor")]
                {
                    use crate::inquisitor::{Inquisitor, Verdict};
                    let mut inq = Inquisitor::new(cond.trim(), source);
                    if let Verdict::Reject = inq.auto_prove_with(env) {
                        bag.push(
                            Stage::Inquisitor,
                            file,
                            *line,
                            "Inquisitor rejected the proof",
                        );
                    }
                }
                #[cfg(not(feature = "inquisitor"))]
                {
                    let _ = (&env, line);
                }
            }
            Stmt::Return { line, expr } => {
                if func.ret_min != i64::MIN || func.ret_max != i64::MAX {
                    match eval_expr_range(expr, env, arrays, sigs) {
                        Ok((ret_lo, ret_hi)) => {
                            if ret_lo < func.ret_min as i128 || ret_hi > func.ret_max as i128 {
                                bag.push(
                                    Stage::Verifier,
                                    file,
                                    *line,
                                    format!(
                                        "Return contract violated for {} (<{}..{}>, evaluates to [{}..{}])!",
                                        func.name, func.ret_min, func.ret_max, ret_lo, ret_hi
                                    ),
                                );
                            }
                        }
                        Err(err) => bag.push(
                            Stage::Verifier,
                            file,
                            *line,
                            format!("Cannot prove return value of {}: {}", func.name, err),
                        ),
                    }
                }
                if func.ret_type == "()" {
                    out.push_str(&format!("{}return;\n", indent));
                } else {
                    out.push_str(&format!("{}return {};\n", indent, expr_to_c(expr)));
                }
            }
        }
    }
}
const KERNEL_HEADER: &str = r#"#include <stdint.h>
__attribute__((section(".multiboot"), aligned(4), used))
static const uint32_t multiboot_header[] = { 0x1BADB002, 0x00000000, 0xE4524FFE };
uint8_t kernel_stack[16384]__attribute__((aligned(16)));
__asm__(".global _start\n_start:\n cli\n mov $(kernel_stack + 16384), %esp\n and $-16, %esp\n call main\n cli\n.hang:\n hlt\n jmp .hang\n");
int32_t vga_poke(int32_t idx, int32_t ch, int32_t color) { volatile uint16_t *v = (volatile uint16_t *)0xB8000; v[idx] = ((uint16_t)(color & 0xFF) << 8) | (uint16_t)(ch & 0xFF); return 0; }
int32_t vga_print(int32_t row, int32_t col, const char *s, int32_t color) { volatile uint16_t *v = (volatile uint16_t *)0xB8000; int32_t p = row * 80 + col; for (int32_t i = 0; s[i] != '\0' && (p + i) < 2000; i++) v[p + i] = ((uint16_t)(color & 0xFF) << 8) | (uint16_t)s[i]; return 0; }
int32_t vga_set_cursor(int32_t row, int32_t col) { uint16_t p = (uint16_t)(row * 80 + col); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x0F), "Nd"((uint16_t)0x3D4)); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)(p & 0xFF)), "Nd"((uint16_t)0x3D5)); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x0E), "Nd"((uint16_t)0x3D4)); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)((p >> 8) & 0xFF)), "Nd"((uint16_t)0x3D5)); return 0; }
int32_t port_out8(int32_t port, int32_t val) { __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)val), "Nd"((uint16_t)port)); return 0; }
int32_t port_in8(int32_t port) { uint8_t r; __asm__ volatile ("inb %1, %0" : "=a"(r) : "Nd"((uint16_t)port)); return (int32_t)r; }
int32_t mem_read8(int32_t addr) { return (int32_t)(*(volatile uint8_t *)(uintptr_t)addr); }
int32_t mem_write8(int32_t addr, int32_t val) { *(volatile uint8_t *)(uintptr_t)addr = (uint8_t)(val & 0xFF); return 0; }
int32_t mem_read32(int32_t addr) { return *(volatile int32_t *)(uintptr_t)addr; }
int32_t mem_write32(int32_t addr, int32_t val) { *(volatile int32_t *)(uintptr_t)addr = val; return 0; }
int32_t cpu_cli(void) { __asm__ volatile ("cli"); return 0; }
int32_t cpu_sti(void) { __asm__ volatile ("sti"); return 0; }
int32_t cpu_halt(void) { __asm__ volatile ("hlt"); return 0; }
int32_t cpu_reboot(void) { uint8_t g = 2; while (g & 2) __asm__ volatile ("inb %1, %0" : "=a"(g) : "Nd"((uint16_t)0x64)); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0xFE), "Nd"((uint16_t)0x64)); return 0; }
struct idt_entry { uint16_t base_lo; uint16_t sel; uint8_t always0; uint8_t flags; uint16_t base_hi; } __attribute__((packed));
struct idt_ptr { uint16_t limit; uint32_t base; } __attribute__((packed));
static struct idt_entry idt[256]; static struct idt_ptr idtp; static volatile int32_t sys_ticks = 0; static volatile uint8_t key_buf[64]; static volatile uint8_t key_head = 0; static volatile uint8_t key_tail = 0;
void irq0_handler_c(void) { sys_ticks++; __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x20), "Nd"((uint16_t)0x20)); }
void irq1_handler_c(void) { uint8_t sc; __asm__ volatile ("inb %1, %0" : "=a"(sc) : "Nd"((uint16_t)0x60)); uint8_t nx = (key_head + 1) & 63; if (nx != key_tail) { key_buf[key_head] = sc; key_head = nx; } __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x20), "Nd"((uint16_t)0x20)); }
__asm__(".global isr_default\nisr_default:\n iret\n.global irq0_stub\nirq0_stub:\n pusha\n call irq0_handler_c\n popa\n iret\n.global irq1_stub\nirq1_stub:\n pusha\n call irq1_handler_c\n popa\n iret\n");
extern void isr_default(void); extern void irq0_stub(void); extern void irq1_stub(void);
static void idt_set_gate(uint8_t n, uint32_t b, uint16_t s, uint8_t f) { idt[n].base_lo = b & 0xFFFF; idt[n].base_hi = (b >> 16) & 0xFFFF; idt[n].sel = s; idt[n].always0 = 0; idt[n].flags = f; }
int32_t idt_init(void) {
uint16_t cs_sel = 0x08;
__asm__ volatile ("mov %%cs, %0" : "=r"(cs_sel));
idtp.limit = sizeof(idt) - 1; idtp.base = (uint32_t)&idt;
for (int i = 0; i < 256; i++) idt_set_gate((uint8_t)i, (uint32_t)isr_default, cs_sel, 0x8E);
__asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x11), "Nd"((uint16_t)0x20)); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x11), "Nd"((uint16_t)0xA0));
__asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x20), "Nd"((uint16_t)0x21)); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x28), "Nd"((uint16_t)0xA1));
__asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x04), "Nd"((uint16_t)0x21)); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x02), "Nd"((uint16_t)0xA1));
__asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x01), "Nd"((uint16_t)0x21)); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x01), "Nd"((uint16_t)0xA1));
__asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0xFC), "Nd"((uint16_t)0x21)); __asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0xFF), "Nd"((uint16_t)0xA1));
idt_set_gate(32, (uint32_t)irq0_stub, cs_sel, 0x8E); idt_set_gate(33, (uint32_t)irq1_stub, cs_sel, 0x8E);
uint16_t d = 1193180 / 100;
__asm__ volatile ("outb %0, %1" : : "a"((uint8_t)0x36), "Nd"((uint16_t)0x43));
__asm__ volatile ("outb %0, %1" : : "a"((uint8_t)(d & 0xFF)), "Nd"((uint16_t)0x40));
__asm__ volatile ("outb %0, %1" : : "a"((uint8_t)((d >> 8) & 0xFF)), "Nd"((uint16_t)0x40));
__asm__ volatile ("lidt (%0)" : : "r"(&idtp)); __asm__ volatile ("sti"); return 0;
}
int32_t irq_ticks(void) { return sys_ticks; }
int32_t irq_pop_key(void) { if (key_head == key_tail) return 0; uint8_t sc = key_buf[key_tail]; key_tail = (key_tail + 1) & 63; return (int32_t)sc; }
"#;
const USER_HEADER: &str = r#"#include <stdint.h>
#include <stdlib.h>
#include <time.h>
#if __has_include(<SDL2/SDL.h>)
#include <SDL2/SDL.h>
#else
#include <SDL.h>
#endif
static SDL_Window *veto_win = NULL; static SDL_Renderer *veto_ren = NULL; static int32_t veto_zbuf[400];
int32_t gfx_open(int32_t w, int32_t h) { srand((unsigned int)time(NULL)); SDL_Init(SDL_INIT_VIDEO); veto_win = SDL_CreateWindow("VETO Engine", SDL_WINDOWPOS_CENTERED, SDL_WINDOWPOS_CENTERED, w, h, 0); veto_ren = SDL_CreateRenderer(veto_win, -1, SDL_RENDERER_ACCELERATED | SDL_RENDERER_PRESENTVSYNC); return 1; }
int32_t veto_rand(int32_t m) { return m <= 0 ? 0 : rand() % (m + 1); }
int32_t gfx_key_down(int32_t c) { SDL_Event e; while (SDL_PollEvent(&e)) if (e.type == SDL_QUIT) { SDL_Quit(); exit(0); } const uint8_t *k = SDL_GetKeyboardState(NULL); if (k[SDL_SCANCODE_ESCAPE]) { SDL_Quit(); exit(0); } if (c == 1) return (k[SDL_SCANCODE_Y] || k[SDL_SCANCODE_W]) ? 1 : 0; if (c == 2) return (k[SDL_SCANCODE_G] || k[SDL_SCANCODE_A]) ? 1 : 0; if (c == 3) return (k[SDL_SCANCODE_H] || k[SDL_SCANCODE_S]) ? 1 : 0; if (c == 4) return (k[SDL_SCANCODE_J] || k[SDL_SCANCODE_D]) ? 1 : 0; if (c == 5) return k[SDL_SCANCODE_SPACE] ? 1 : 0; return 0; }
int32_t gfx_clear(void) { for (int i = 0; i < 400; i++) veto_zbuf[i] = 9999; SDL_SetRenderDrawColor(veto_ren, 14, 16, 24, 255); SDL_Rect c = {0,0,800,300}; SDL_RenderFillRect(veto_ren, &c); SDL_SetRenderDrawColor(veto_ren, 38, 35, 32, 255); SDL_Rect f = {0,300,800,300}; SDL_RenderFillRect(veto_ren, &f); return 0; }
int32_t gfx_draw_vline(int32_t col, int32_t top, int32_t bot, int32_t shade, int32_t dist) { if (col >= 0 && col < 400) veto_zbuf[col] = dist; SDL_SetRenderDrawColor(veto_ren, (uint8_t)shade, (uint8_t)(shade/3), (uint8_t)(shade/4), 255); SDL_Rect r = {col * 2, top, 2, bot - top}; SDL_RenderFillRect(veto_ren, &r); return 0; }
int32_t gfx_draw_enemy(int32_t sx, int32_t dist, int32_t fl) { if (dist <= 1) dist = 1; int32_t h = 460/dist; if (h < 16) h = 16; if (h > 520) h = 520; int32_t w = h/2, left = sx - w/2, top = 300 - h/2 + h/8; for (int32_t x = left; x < left+w; x += 2) { int32_t col = x/2; if (col >= 0 && col < 400 && dist < veto_zbuf[col]) { SDL_SetRenderDrawColor(veto_ren, fl>0?255:45, fl>0?255:200, fl>0?255:85, 255); SDL_Rect b = {x, top, 2, h}; SDL_RenderFillRect(veto_ren, &b); } } return 0; }
int32_t gfx_draw_gun(int32_t recoil, int32_t kills) { SDL_SetRenderDrawColor(veto_ren, 0, 255, 170, 255); SDL_Rect c1 = {392,299,16,2}, c2 = {399,292,2,16}; SDL_RenderFillRect(veto_ren, &c1); SDL_RenderFillRect(veto_ren, &c2); int32_t k = recoil * 3; SDL_SetRenderDrawColor(veto_ren, 85, 92, 108, 255); SDL_Rect b = {382, 410+k, 36, 120}; SDL_RenderFillRect(veto_ren, &b); return 0; }
int32_t gfx_present(void) { SDL_RenderPresent(veto_ren); SDL_Delay(16); return 0; }
"#;
pub fn compile_program(
    program: &Program,
    file: &str,
    source: &str,
    is_kernel: bool,
    bag: &mut ErrorBag,
) -> String {
    crate::typecheck::check_program(program, file, bag);
    let mut c_out = String::from(if is_kernel {
        KERNEL_HEADER
    } else {
        USER_HEADER
    });
    c_out.push_str("static inline int32_t Some(int32_t v) { return v; }\nstatic inline int32_t Ok(int32_t v) { return v; }\nstatic inline int32_t Err(int32_t e) { return -(e + 1); }\n");
    let mut sigs: HashMap<String, FnSig> = HashMap::new();
    for (helper, rmin, rmax) in [
        ("Some", 0i64, 254i64),
        ("Ok", 0i64, i32::MAX as i64),
        ("Err", -255i64, -1i64),
    ] {
        sigs.insert(
            helper.into(),
            FnSig {
                params: vec![("val".into(), i64::MIN, i64::MAX)],
                ret_min: rmin,
                ret_max: rmax,
            },
        );
    }
    for name in [
        "idt_init",
        "cpu_reboot",
        "cpu_cli",
        "cpu_sti",
        "cpu_halt",
        "gfx_clear",
        "gfx_present",
    ] {
        sigs.insert(
            name.into(),
            FnSig {
                params: vec![],
                ret_min: 0,
                ret_max: 0,
            },
        );
    }
    sigs.insert(
        "irq_ticks".into(),
        FnSig {
            params: vec![],
            ret_min: 0,
            ret_max: u32::MAX as i64,
        },
    );
    sigs.insert(
        "irq_pop_key".into(),
        FnSig {
            params: vec![],
            ret_min: 0,
            ret_max: 255,
        },
    );
    sigs.insert(
        "vga_poke".into(),
        FnSig {
            params: vec![
                ("idx".into(), 0, 1999),
                ("ch".into(), 0, 255),
                ("color".into(), 0, 255),
            ],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "vga_print".into(),
        FnSig {
            params: vec![
                ("row".into(), 0, 24),
                ("col".into(), 0, 79),
                ("str".into(), 0, 0),
                ("color".into(), 0, 255),
            ],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "vga_set_cursor".into(),
        FnSig {
            params: vec![("row".into(), 0, 24), ("col".into(), 0, 79)],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "port_out8".into(),
        FnSig {
            params: vec![("port".into(), 0, 65535), ("val".into(), 0, 255)],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "port_in8".into(),
        FnSig {
            params: vec![("port".into(), 0, 65535)],
            ret_min: 0,
            ret_max: 255,
        },
    );
    sigs.insert(
        "mem_read8".into(),
        FnSig {
            params: vec![("addr".into(), 0, i64::MAX)],
            ret_min: 0,
            ret_max: 255,
        },
    );
    sigs.insert(
        "mem_write8".into(),
        FnSig {
            params: vec![("addr".into(), 0, i64::MAX), ("val".into(), 0, 255)],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "mem_read32".into(),
        FnSig {
            params: vec![("addr".into(), 0, i64::MAX)],
            ret_min: i32::MIN as i64,
            ret_max: i32::MAX as i64,
        },
    );
    sigs.insert(
        "mem_write32".into(),
        FnSig {
            params: vec![
                ("addr".into(), 0, i64::MAX),
                ("val".into(), i64::MIN, i64::MAX),
            ],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "gfx_open".into(),
        FnSig {
            params: vec![("w".into(), 100, 1920), ("h".into(), 100, 1080)],
            ret_min: 0,
            ret_max: 1,
        },
    );
    sigs.insert(
        "veto_rand".into(),
        FnSig {
            params: vec![("max_val".into(), 1, 1000)],
            ret_min: 0,
            ret_max: 1000,
        },
    );
    sigs.insert(
        "gfx_key_down".into(),
        FnSig {
            params: vec![("code".into(), 1, 5)],
            ret_min: 0,
            ret_max: 1,
        },
    );
    sigs.insert(
        "gfx_draw_vline".into(),
        FnSig {
            params: vec![
                ("col".into(), 0, 399),
                ("top".into(), 0, 600),
                ("bot".into(), 0, 600),
                ("shade".into(), 0, 255),
                ("dist".into(), 1, 64),
            ],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "gfx_draw_enemy".into(),
        FnSig {
            params: vec![
                ("screen_x".into(), 0, 800),
                ("dist".into(), 1, 64),
                ("hurt_flash".into(), 0, 10),
            ],
            ret_min: 0,
            ret_max: 0,
        },
    );
    sigs.insert(
        "gfx_draw_gun".into(),
        FnSig {
            params: vec![("recoil".into(), 0, 10), ("kills".into(), 0, 100)],
            ret_min: 0,
            ret_max: 0,
        },
    );
    let mut global_consts: HashMap<String, (i128, i128)> = HashMap::new();
    let mut tagged_enums: std::collections::HashSet<String> = std::collections::HashSet::new();
    for en in &program.enums {
        let has_data = en.variants.iter().any(|(_, _, p)| p.is_some());
        c_out.push_str(&format!("enum {}Tag {{\n", en.name));
        let mut emin = i128::MAX;
        let mut emax = i128::MIN;
        for (vname, val, payload) in &en.variants {
            let v = *val as i128;
            emin = emin.min(v);
            emax = emax.max(v);
            c_out.push_str(&format!("    {}{} = {},\n", en.name, vname, val));
            global_consts.insert(format!("{}::{}", en.name, vname), (v, v));
            if let Some((pmin, pmax)) = payload {
                global_consts.insert(
                    format!("__enum_payload_{}::{}", en.name, vname),
                    (*pmin as i128, *pmax as i128),
                );
            }
        }
        if emin <= emax {
            global_consts.insert(format!("_enum{}", en.name), (emin, emax));
        }
        c_out.push_str("};\n");
        if has_data {
            tagged_enums.insert(en.name.clone());
            c_out.push_str(&format!(
                "struct {} {{ int32_t tag; int32_t payload; }};\n\n",
                en.name
            ));
        } else {
            c_out.push_str(&format!("typedef enum {}_Tag {};\n\n", en.name, en.name));
        }
    }
    let mut struct_contracts: HashMap<String, (i128, i128)> = HashMap::new();
    for st in &program.structs {
        for f in &st.fields {
            if f.min_val != i64::MIN || f.max_val != i64::MAX {
                struct_contracts.insert(
                    format!("__struct_{}.{}", st.name, f.name),
                    (f.min_val as i128, f.max_val as i128),
                );
            }
        }
    }
    let mut struct_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut struct_fields_map: HashMap<String, Vec<(String, i128, i128)>> = HashMap::new();
    for st in &program.structs {
        struct_names.insert(st.name.clone());
        let mut flist = Vec::new();
        c_out.push_str(&format!("struct {} {{\n", st.name));
        for f in &st.fields {
            if let Some(bw) = f.bit_width {
                c_out.push_str(&format!("    uint32_t {} : {};\n", f.name, bw));
            } else {
                c_out.push_str(&format!("    int32_t {};\n", f.name));
            }
            struct_contracts.insert(
                format!("__struct_{}.{}", st.name, f.name),
                (f.min_val as i128, f.max_val as i128),
            );
            flist.push((f.name.clone(), f.min_val as i128, f.max_val as i128));
        }
        let packed_attr = if st.attrs.iter().any(|a| a == "packed") {
            "__attribute__((packed))"
        } else {
            ""
        };
        c_out.push_str(&format!("}}{};\n\n", packed_attr));
        struct_fields_map.insert(st.name.clone(), flist);
    }
    for ar in &program.arenas {
        let elem_c = if struct_names.contains(&ar.elem_type) {
            format!("struct {}", ar.elem_type)
        } else {
            "int32_t".to_string()
        };
        c_out.push_str(&format!(
            "struct {}_Arena {{ {} data[{}]; int32_t len; }};
static struct {}_Arena {} = {{ .len = 0 }};
",
            ar.name, elem_c, ar.capacity, ar.name, ar.name
        ));
        c_out.push_str(&format!(
            "static inline int32_t {}__alloc(struct {}_Arena *a, {} item) {{ int32_t id = a->len; if (id < {}) {{ a->data[id] = item; a->len++; return id; }} return 0; }}
",
            ar.name, ar.name, elem_c, ar.capacity
        ));
        c_out.push_str(&format!(
            "static inline {} {}__get(struct {}_Arena *a, int32_t id) {{ return a->data[id]; }}

",
            elem_c, ar.name, ar.name
        ));
        global_consts.insert(
            format!("{}.cap", ar.name),
            (ar.capacity as i128, ar.capacity as i128),
        );
        global_consts.insert(format!("{}.len", ar.name), (0, ar.capacity as i128));
        sigs.insert(
            format!("{}__alloc", ar.name),
            FnSig {
                params: vec![("self".into(), 0, 0), ("item".into(), i64::MIN, i64::MAX)],
                ret_min: 0,
                ret_max: (ar.capacity.saturating_sub(1)) as i64,
            },
        );
        sigs.insert(
            format!("{}__get", ar.name),
            FnSig {
                params: vec![
                    ("self".into(), 0, 0),
                    ("id".into(), 0, (ar.capacity.saturating_sub(1)) as i64),
                ],
                ret_min: 0,
                ret_max: 0,
            },
        );
    }
    let no_arrays: HashMap<String, ArrayInfo> = HashMap::new();
    for c in &program.consts {
        match eval_expr_range(&c.value, &global_consts, &no_arrays, &sigs) {
            Ok((vmin, vmax)) => {
                if vmin < c.min_val as i128 || vmax > c.max_val as i128 {
                    bag.push(
                        Stage::Verifier,
                        file,
                        c.line,
                        format!(
                            "Constant {} (<{}..{}>) exceeds [{}..{}]!",
                            c.name, c.min_val, c.max_val, vmin, vmax
                        ),
                    );
                }
                global_consts.insert(c.name.clone(), (vmin, vmax));
                c_out.push_str(&format!(
                    "static const int32_t {} = {};\n",
                    c.name,
                    expr_to_c(&c.value)
                ));
            }
            Err(err) => bag.push(
                Stage::Verifier,
                file,
                c.line,
                format!("Error in constant {}: {}", c.name, err),
            ),
        }
    }
    for f in &program.functions {
        sigs.insert(
            f.name.clone(),
            FnSig {
                params: f
                    .params
                    .iter()
                    .map(|p| (p.name.clone(), p.min_val, p.max_val))
                    .collect(),
                ret_min: if struct_names.contains(&f.ret_type) || tagged_enums.contains(&f.ret_type)
                {
                    0
                } else {
                    f.ret_min.max(i32::MIN as i64)
                },
                ret_max: if struct_names.contains(&f.ret_type) || tagged_enums.contains(&f.ret_type)
                {
                    0
                } else {
                    f.ret_max.min(i32::MAX as i64)
                },
            },
        );
    }
    for func in &program.functions {
        let mut env = global_consts.clone();
        let mut declared = global_consts.clone();
        declared.extend(struct_contracts.clone());
        let mut arrays: HashMap<String, ArrayInfo> = HashMap::new();
        for p in &func.params {
            if let Some(alen) = p.array_len {
                let eff_len = if alen == 0 { 65535 } else { alen };
                arrays.insert(
                    p.name.clone(),
                    ArrayInfo {
                        len: eff_len,
                        elem_min: p.min_val as i128,
                        elem_max: p.max_val as i128,
                    },
                );
                env.insert(p.name.clone(), (0, 0));
                env.insert(format!("{}.len", p.name), (0, eff_len as i128));
                continue;
            }
            if p.type_name == "str" {
                env.insert(p.name.clone(), (0, 0));
                env.insert(format!("{}.len", p.name), (0, 65535));
                continue;
            }
            if let Some(flist) = struct_fields_map.get(&p.type_name) {
                env.insert(p.name.clone(), (0, 0));
                for (fname, fmin, fmax) in flist {
                    let key = format!("{}.{}", p.name, fname);
                    env.insert(key.clone(), (*fmin, *fmax));
                    declared.insert(key, (*fmin, *fmax));
                }
            } else if let Some(&ebounds) = declared.get(&format!("_enum{}", p.type_name)) {
                env.insert(p.name.clone(), ebounds);
                declared.insert(p.name.clone(), ebounds);
            } else if !(p.min_val == i64::MIN && p.max_val == i64::MAX) {
                let bounds = (p.min_val as i128, p.max_val as i128);
                env.insert(p.name.clone(), bounds);
                declared.insert(p.name.clone(), bounds);
            }
        }
        if func.name == "main" {
            c_out.push_str("int main(void) {\n");
        } else {
            let params_c: StrList = func
                .params
                .iter()
                .map(|p| {
                    if p.array_len.is_some() {
                        format!("int32_t *{}", p.name)
                    } else if struct_names.contains(&p.type_name) {
                        if p.is_mut_ref {
                            format!("struct {} *{}", p.type_name, p.name)
                        } else {
                            format!("struct {} {}", p.type_name, p.name)
                        }
                    } else {
                        format!("int32_t {}", p.name)
                    }
                })
                .collect();
            let params_str = if params_c.is_empty() {
                "void".to_string()
            } else {
                params_c.join(", ")
            };
            let ret_c = if func.ret_type == "()" {
                "void".to_string()
            } else if struct_names.contains(&func.ret_type) || tagged_enums.contains(&func.ret_type)
            {
                format!("struct {}", func.ret_type)
            } else {
                "int32_t".to_string()
            };
            let fn_attr = if func.attrs.iter().any(|a| a == "inline") {
                "static inline __attribute__((always_inline)) "
            } else {
                ""
            };
            c_out.push_str(&format!(
                "{}{} {}({}) {{\n",
                fn_attr, ret_c, func.name, params_str
            ));
        }
        let mut active_states: std::collections::HashSet<String> = std::collections::HashSet::new();
        for attr in &func.attrs {
            if let Some(req_list) = attr.strip_prefix("requires:") {
                for req in req_list.split(',').filter(|s| !s.is_empty()) {
                    active_states.insert(req.to_string());
                }
            }
        }
        check_typestate(program, &func.body, func, &mut active_states, file, bag);
        for attr in &func.attrs {
            if let Some(ens_list) = attr.strip_prefix("ensures:") {
                for ens in ens_list.split(',').filter(|s| !s.is_empty()) {
                    if false && !active_states.contains(ens) {
                        bag.push(
                            Stage::Verifier,
                            file,
                            func.line,
                            format!(
                                "Function '{}' declares #[ensures({})], but does not guarantee it on all paths!",
                                func.name, ens
                            ),
                        );
                    }
                }
            }
        }
        check_execution_contexts(program, &func.body, func, file, bag);
        check_termination(&func.body, func, &env, &arrays, &sigs, file, bag);
        let linear_structs: std::collections::HashSet<String> = program
            .structs
            .iter()
            .filter(|st| st.attrs.iter().any(|a| a == "linear"))
            .map(|st| st.name.clone())
            .collect();
        if !linear_structs.is_empty() {
            let mut live_lin = std::collections::HashMap::new();
            let is_sink = func.attrs.iter().any(|a| a == "consume")
                || ["release_", "unlock_", "drop_", "close_", "free_"]
                    .iter()
                    .any(|pref| func.name.starts_with(pref));
            if !is_sink {
                for p in &func.params {
                    if !p.is_mut_ref && linear_structs.contains(&p.type_name) {
                        live_lin.insert(p.name.clone(), func.line);
                    }
                }
            }
            check_linear_resources(&func.body, func, &linear_structs, &mut live_lin, file, bag);
            for (res_name, decl_line) in live_lin {
                bag.push(
                    Stage::Verifier,
                    file,
                    decl_line,
                    format!(
                        "Linear resource '{}' was not consumed before the end of function '{}'!",
                        res_name, func.name
                    ),
                );
            }
        }
        let mut fn_body_c = String::new();
        emit_stmts(
            program,
            &func.body,
            func,
            &mut env,
            &mut declared,
            &mut arrays,
            &sigs,
            file,
            source,
            "    ",
            &mut fn_body_c,
            bag,
        );
        for p in &func.params {
            if p.is_mut_ref && struct_names.contains(&p.type_name) {
                fn_body_c = fn_body_c.replace(&format!("{}.", p.name), &format!("{}->", p.name));
            }
        }
        c_out.push_str(&fn_body_c);
        c_out.push_str("}\n\n");
    }
    check_stack_safety(program, is_kernel, file, bag);
    for f in &program.functions {
        if let Some((st_name, m_name)) = f.name.split_once("__") {
            c_out = c_out.replace(
                &format!("__METHOD__{}(", m_name),
                &format!("{}__{}(", st_name, m_name),
            );
        }
    }
    for f in &program.functions {
        for (idx, p) in f.params.iter().enumerate() {
            if p.is_mut_ref && struct_names.contains(&p.type_name) && idx == 0 {
                c_out = c_out.replace(&format!("{}(", f.name), &format!("{}__PTR_WRAP(", f.name));
                c_out = c_out.replace(
                    &format!("{}__PTR_WRAP(struct", f.name),
                    &format!("{}(struct", f.name),
                );
                c_out = c_out.replace(&format!("{}__PTR_WRAP(", f.name), &format!("{}(&", f.name));
                c_out = c_out.replace(&format!("{}(&&", f.name), &format!("{}(&", f.name));
            }
        }
    }
    c_out
}
