use crate::ast::*;
use crate::error::{ErrorBag, Stage};
use std::collections::{HashMap, HashSet};

pub type StructMap = HashMap<String, HashMap<String, VetoType>>;
pub type FnSigMap = HashMap<String, (Vec<(VetoType, bool)>, VetoType)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VetoType {
    Int,
    Str,
    Enum(String),
    Struct(String),
    Array(usize),
}

impl VetoType {
    pub fn from_name(name: &str, structs: &StructMap, enums: &HashSet<String>) -> Self {
        if name == "str" {
            VetoType::Str
        } else if enums.contains(name) {
            VetoType::Enum(name.to_string())
        } else if structs.contains_key(name) {
            VetoType::Struct(name.to_string())
        } else {
            VetoType::Int
        }
    }

    pub fn label(&self) -> String {
        match self {
            VetoType::Int => "int (i32/u8)".to_string(),
            VetoType::Str => "str".to_string(),
            VetoType::Enum(e) => format!("enum {}", e),
            VetoType::Struct(s) => format!("struct {}", s),
            VetoType::Array(n) => format!("array[{}]", n),
        }
    }
}

struct ScopeStack {
    frames: Vec<HashMap<String, VetoType>>,
}

impl ScopeStack {
    fn new() -> Self {
        Self {
            frames: vec![HashMap::new()],
        }
    }
    fn enter(&mut self) {
        self.frames.push(HashMap::new());
    }
    fn exit(&mut self) {
        if self.frames.len() > 1 {
            self.frames.pop();
        }
    }
    fn insert(&mut self, name: String, ty: VetoType) {
        if let Some(top) = self.frames.last_mut() {
            top.insert(name, ty);
        }
    }
    fn lookup(&self, name: &str) -> Option<&VetoType> {
        for frame in self.frames.iter().rev() {
            if let Some(ty) = frame.get(name) {
                return Some(ty);
            }
        }
        None
    }
}

pub fn check_program(prog: &Program, file: &str, bag: &mut ErrorBag) {
    let mut enums = HashSet::new();
    let mut enum_variants_map: HashMap<String, HashMap<String, bool>> = HashMap::new();
    for en in &prog.enums {
        if !enums.insert(en.name.clone()) {
            bag.push(
                Stage::TypeCheck,
                file,
                en.line,
                format!("duplicate enum '{}'", en.name),
            );
        }
        let mut vmap = HashMap::new();
        for (vname, _, payload) in &en.variants {
            vmap.insert(vname.clone(), payload.is_some());
        }
        enum_variants_map.insert(en.name.clone(), vmap);
    }

    let mut structs: StructMap = HashMap::new();
    for st in &prog.structs {
        if structs.contains_key(&st.name) {
            bag.push(
                Stage::TypeCheck,
                file,
                st.line,
                format!("duplicate struct '{}'", st.name),
            );
        }
        let mut fields = HashMap::new();
        for f in &st.fields {
            if fields.contains_key(&f.name) {
                bag.push(
                    Stage::TypeCheck,
                    file,
                    st.line,
                    format!("duplicate field '{}.{}'", st.name, f.name),
                );
            }
            fields.insert(f.name.clone(), VetoType::Int);
        }
        structs.insert(st.name.clone(), fields);
    }

    let mut fn_sigs: FnSigMap = HashMap::new();
    for helper in ["Some", "Ok", "Err"] {
        fn_sigs.insert(helper.into(), (vec![(VetoType::Int, false)], VetoType::Int));
    }
    for ar in &prog.arenas {
        let item_ty = if structs.contains_key(&ar.elem_type) {
            VetoType::Struct(ar.elem_type.clone())
        } else {
            VetoType::Int
        };
        fn_sigs.insert(
            format!("{}__alloc", ar.name),
            (
                vec![
                    (VetoType::Struct(ar.name.clone()), false),
                    (item_ty.clone(), false),
                ],
                VetoType::Int,
            ),
        );
        fn_sigs.insert(
            format!("{}__get", ar.name),
            (
                vec![
                    (VetoType::Struct(ar.name.clone()), false),
                    (VetoType::Int, false),
                ],
                item_ty,
            ),
        );
    }
    for name in [
        "idt_init",
        "cpu_reboot",
        "cpu_cli",
        "cpu_sti",
        "cpu_halt",
        "irq_ticks",
        "irq_pop_key",
        "gfx_clear",
        "gfx_present",
    ] {
        fn_sigs.insert(name.into(), (vec![], VetoType::Int));
    }
    for (name, argc) in [
        ("vga_poke", 3),
        ("vga_print", 4),
        ("vga_set_cursor", 2),
        ("port_out8", 2),
        ("port_in8", 1),
        ("mem_read8", 1),
        ("mem_write8", 2),
        ("mem_read32", 1),
        ("mem_write32", 2),
        ("gfx_open", 2),
        ("veto_rand", 1),
        ("gfx_key_down", 1),
        ("gfx_draw_vline", 5),
        ("gfx_draw_enemy", 3),
        ("gfx_draw_gun", 2),
    ] {
        fn_sigs.insert(
            name.into(),
            (vec![(VetoType::Int, false); argc], VetoType::Int),
        );
    }

    for f in &prog.functions {
        if fn_sigs.contains_key(&f.name) {
            bag.push(
                Stage::TypeCheck,
                file,
                f.line,
                format!("redeclaration of function '{}'", f.name),
            );
        }
        let ptypes = f
            .params
            .iter()
            .map(|p| {
                let t = if let Some(len) = p.array_len {
                    VetoType::Array(len)
                } else {
                    VetoType::from_name(&p.type_name, &structs, &enums)
                };
                (t, p.is_mut_ref)
            })
            .collect();
        let rtype = VetoType::from_name(&f.ret_type, &structs, &enums);
        fn_sigs.insert(f.name.clone(), (ptypes, rtype));
    }

    let mut globals = HashMap::new();
    for c in &prog.consts {
        globals.insert(c.name.clone(), VetoType::Int);
    }

    for f in &prog.functions {
        let mut scopes = ScopeStack::new();
        for ar in &prog.arenas {
            scopes.insert(ar.name.clone(), VetoType::Struct(ar.name.clone()));
            let mut am = HashMap::new();
            am.insert("len".into(), VetoType::Int);
            am.insert("cap".into(), VetoType::Int);
            structs.insert(ar.name.clone(), am);
        }
        for (k, v) in &globals {
            scopes.insert(k.clone(), v.clone());
        }
        for p in &f.params {
            let pty = if let Some(len) = p.array_len {
                VetoType::Array(len)
            } else {
                VetoType::from_name(&p.type_name, &structs, &enums)
            };
            scopes.insert(p.name.clone(), pty);
        }
        let ret_ty = VetoType::from_name(&f.ret_type, &structs, &enums);
        check_stmts(
            &f.body,
            file,
            0,
            &ret_ty,
            &mut scopes,
            &structs,
            &enums,
            &fn_sigs,
            bag,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn check_stmts(
    stmts: &[Stmt],
    file: &str,
    loop_depth: usize,
    expected_ret: &VetoType,
    scopes: &mut ScopeStack,
    structs: &StructMap,
    enums: &HashSet<String>,
    fn_sigs: &FnSigMap,
    bag: &mut ErrorBag,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                line,
                name,
                type_name,
                value,
                ..
            } => {
                let declared_ty = VetoType::from_name(type_name, structs, enums);
                let actual_ty = infer_expr(value, file, *line, scopes, structs, fn_sigs, bag);
                if declared_ty != actual_ty {
                    bag.push(
                        Stage::TypeCheck,
                        file,
                        *line,
                        format!(
                            "type mismatch for '{}': expected {}, got {}",
                            name,
                            declared_ty.label(),
                            actual_ty.label()
                        ),
                    );
                }
                scopes.insert(name.clone(), declared_ty);
            }
            Stmt::LetArray {
                line,
                name,
                len,
                elements,
                ..
            } => {
                for el in elements {
                    let t = infer_expr(el, file, *line, scopes, structs, fn_sigs, bag);
                    if t != VetoType::Int {
                        bag.push(
                            Stage::TypeCheck,
                            file,
                            *line,
                            format!("array element of '{}' must be an integer", name),
                        );
                    }
                }
                scopes.insert(name.clone(), VetoType::Array(*len));
            }
            Stmt::Assign { line, name, value } => {
                let actual_ty = infer_expr(value, file, *line, scopes, structs, fn_sigs, bag);
                match scopes.lookup(name) {
                    Some(expected) if *expected != actual_ty => bag.push(
                        Stage::TypeCheck,
                        file,
                        *line,
                        format!(
                            "cannot assign {} to '{}' of type {}",
                            actual_ty.label(),
                            name,
                            expected.label()
                        ),
                    ),
                    None => bag.push(
                        Stage::TypeCheck,
                        file,
                        *line,
                        format!("undeclared variable '{}'", name),
                    ),
                    _ => {}
                }
            }
            Stmt::FieldAssign {
                line,
                obj,
                field,
                value,
            } => {
                let actual_ty = infer_expr(value, file, *line, scopes, structs, fn_sigs, bag);
                match scopes.lookup(obj) {
                    Some(VetoType::Struct(st_name)) => {
                        if let Some(fields) = structs.get(st_name) {
                            if !fields.contains_key(field) {
                                bag.push(
                                    Stage::TypeCheck,
                                    file,
                                    *line,
                                    format!("struct '{}' has no field '{}'", st_name, field),
                                );
                            } else if actual_ty != VetoType::Int {
                                bag.push(
                                    Stage::TypeCheck,
                                    file,
                                    *line,
                                    format!("field '{}.{}' expects an integer", obj, field),
                                );
                            }
                        }
                    }
                    Some(other) => bag.push(
                        Stage::TypeCheck,
                        file,
                        *line,
                        format!(
                            "'{}' has type {}, cannot access field '{}'",
                            obj,
                            other.label(),
                            field
                        ),
                    ),
                    None => bag.push(
                        Stage::TypeCheck,
                        file,
                        *line,
                        format!("undeclared struct '{}'", obj),
                    ),
                }
            }
            Stmt::IndexAssign {
                line,
                name,
                index,
                value,
            } => {
                let idx_ty = infer_expr(index, file, *line, scopes, structs, fn_sigs, bag);
                let val_ty = infer_expr(value, file, *line, scopes, structs, fn_sigs, bag);
                if idx_ty != VetoType::Int || val_ty != VetoType::Int {
                    bag.push(
                        Stage::TypeCheck,
                        file,
                        *line,
                        "array index and value must be integers",
                    );
                }
                match scopes.lookup(name) {
                    Some(VetoType::Array(_)) => {}
                    Some(other) => bag.push(
                        Stage::TypeCheck,
                        file,
                        *line,
                        format!("'{}' has type {}, cannot index", name, other.label()),
                    ),
                    None => bag.push(
                        Stage::TypeCheck,
                        file,
                        *line,
                        format!("undeclared array '{}'", name),
                    ),
                }
            }
            Stmt::Loop { body, .. } => {
                scopes.enter();
                check_stmts(
                    body,
                    file,
                    loop_depth + 1,
                    expected_ret,
                    scopes,
                    structs,
                    enums,
                    fn_sigs,
                    bag,
                );
                scopes.exit();
            }
            Stmt::While { line, cond, body } => {
                let cty = infer_expr(cond, file, *line, scopes, structs, fn_sigs, bag);
                if cty != VetoType::Int {
                    bag.push(
                        Stage::TypeCheck,
                        file,
                        *line,
                        "while condition must be boolean/integer",
                    );
                }
                scopes.enter();
                check_stmts(
                    body,
                    file,
                    loop_depth + 1,
                    expected_ret,
                    scopes,
                    structs,
                    enums,
                    fn_sigs,
                    bag,
                );
                scopes.exit();
            }
            Stmt::For {
                line,
                var,
                start,
                end,
                body,
            } => {
                let st = infer_expr(start, file, *line, scopes, structs, fn_sigs, bag);
                let en = infer_expr(end, file, *line, scopes, structs, fn_sigs, bag);
                if st != VetoType::Int || en != VetoType::Int {
                    bag.push(
                        Stage::TypeCheck,
                        file,
                        *line,
                        "for loop bounds must be integers",
                    );
                }
                scopes.enter();
                scopes.insert(var.clone(), VetoType::Int);
                check_stmts(
                    body,
                    file,
                    loop_depth + 1,
                    expected_ret,
                    scopes,
                    structs,
                    enums,
                    fn_sigs,
                    bag,
                );
                scopes.exit();
            }
            Stmt::Break { line } => {
                if loop_depth == 0 {
                    bag.push(
                        Stage::TypeCheck,
                        file,
                        *line,
                        "'break' statement outside of loop",
                    );
                }
            }
            Stmt::Continue { line } => {
                if loop_depth == 0 {
                    bag.push(
                        Stage::TypeCheck,
                        file,
                        *line,
                        "'continue' statement outside of loop",
                    );
                }
            }
            Stmt::If {
                line,
                cond,
                then_body,
                else_body,
            } => {
                let cty = infer_expr(cond, file, *line, scopes, structs, fn_sigs, bag);
                if cty != VetoType::Int {
                    bag.push(
                        Stage::TypeCheck,
                        file,
                        *line,
                        "if condition must be boolean/integer",
                    );
                }
                scopes.enter();
                check_stmts(
                    then_body,
                    file,
                    loop_depth,
                    expected_ret,
                    scopes,
                    structs,
                    enums,
                    fn_sigs,
                    bag,
                );
                scopes.exit();
                if !else_body.is_empty() {
                    scopes.enter();
                    check_stmts(
                        else_body,
                        file,
                        loop_depth,
                        expected_ret,
                        scopes,
                        structs,
                        enums,
                        fn_sigs,
                        bag,
                    );
                    scopes.exit();
                }
            }
            Stmt::ExprStmt { line, expr } => {
                let _ = infer_expr(expr, file, *line, scopes, structs, fn_sigs, bag);
            }
            Stmt::Match { line, expr, arms } => {
                let subj_ty = infer_expr(expr, file, *line, scopes, structs, fn_sigs, bag);
                for arm in arms {
                    match &arm.pat {
                        MatchPat::EnumVal(en, _, _) => {
                            if subj_ty != VetoType::Enum(en.clone()) {
                                bag.push(
                                    Stage::TypeCheck,
                                    file,
                                    arm.line,
                                    format!(
                                        "type mismatch in match arm: expected {}, found enum {}",
                                        subj_ty.label(),
                                        en
                                    ),
                                );
                            }
                        }
                        MatchPat::Num(_) | MatchPat::Range(_, _) => {
                            if subj_ty != VetoType::Int {
                                bag.push(
                                    Stage::TypeCheck,
                                    file,
                                    arm.line,
                                    format!(
                                        "numeric match pattern cannot be applied to {}",
                                        subj_ty.label()
                                    ),
                                );
                            }
                        }
                        MatchPat::Wildcard => {}
                    }
                    scopes.enter();
                    if let MatchPat::EnumVal(_, _, Some(bind_name)) = &arm.pat {
                        scopes.insert(bind_name.clone(), VetoType::Int);
                    }
                    check_stmts(
                        &arm.body,
                        file,
                        loop_depth,
                        expected_ret,
                        scopes,
                        structs,
                        enums,
                        fn_sigs,
                        bag,
                    );
                    scopes.exit();
                }
            }
            Stmt::Prove { .. } | Stmt::InlineAsm { .. } => {}
            Stmt::Return { line, expr } => {
                let actual = infer_expr(expr, file, *line, scopes, structs, fn_sigs, bag);
                if actual != *expected_ret {
                    bag.push(
                        Stage::TypeCheck,
                        file,
                        *line,
                        format!(
                            "invalid return type: expected {}, got {}",
                            expected_ret.label(),
                            actual.label()
                        ),
                    );
                }
            }
        }
    }
}
fn infer_expr(
    expr: &Expr,
    file: &str,
    line: usize,
    scopes: &ScopeStack,
    structs: &StructMap,
    fn_sigs: &FnSigMap,
    bag: &mut ErrorBag,
) -> VetoType {
    match expr {
        Expr::Number(..) => VetoType::Int,
        Expr::Str(..) => VetoType::Str,
        Expr::EnumVal(en, var, payload) => {
            if let Some(pexpr) = payload {
                let pt = infer_expr(pexpr, file, line, scopes, structs, fn_sigs, bag);
                if pt != VetoType::Int {
                    bag.push(
                        Stage::TypeCheck,
                        file,
                        line,
                        format!("payload of variant '{}::{}' must be an integer", en, var),
                    );
                }
            }
            VetoType::Enum(en.clone())
        }
        Expr::Var(name) => match scopes.lookup(name) {
            Some(ty) => ty.clone(),
            None => {
                bag.push(
                    Stage::TypeCheck,
                    file,
                    line,
                    format!("undeclared identifier '{}'", name),
                );
                VetoType::Int
            }
        },
        Expr::Field(obj, field) => match scopes.lookup(obj) {
            Some(VetoType::Struct(st_name)) => {
                if let Some(fields) = structs.get(st_name) {
                    if let Some(fty) = fields.get(field) {
                        return fty.clone();
                    }
                }
                bag.push(
                    Stage::TypeCheck,
                    file,
                    line,
                    format!("unknown field '{}.{}'", obj, field),
                );
                VetoType::Int
            }
            Some(VetoType::Array(_)) | Some(VetoType::Str) | Some(VetoType::Int) if field == "len" => VetoType::Int,
            Some(other) => {
                bag.push(
                    Stage::TypeCheck,
                    file,
                    line,
                    format!(
                        "'{}' has type {}, cannot access field '{}'",
                        obj,
                        other.label(),
                        field
                    ),
                );
                VetoType::Int
            }
            None => {
                bag.push(
                    Stage::TypeCheck,
                    file,
                    line,
                    format!("undeclared variable '{}'", obj),
                );
                VetoType::Int
            }
        },
        Expr::StructInit(st_name, inits) => {
            if let Some(fields) = structs.get(st_name) {
                for (fname, fexpr) in inits {
                    let fty = infer_expr(fexpr, file, line, scopes, structs, fn_sigs, bag);
                    if !fields.contains_key(fname) {
                        bag.push(
                            Stage::TypeCheck,
                            file,
                            line,
                            format!("struct '{}' has no field '{}'", st_name, fname),
                        );
                    } else if fty != VetoType::Int {
                        bag.push(
                            Stage::TypeCheck,
                            file,
                            line,
                            format!("field '{}.{}' requires an integer", st_name, fname),
                        );
                    }
                }
                if inits.len() != fields.len() {
                    bag.push(
                        Stage::TypeCheck,
                        file,
                        line,
                        format!(
                            "struct '{}' has {} fields, initialized {}",
                            st_name,
                            fields.len(),
                            inits.len()
                        ),
                    );
                }
                VetoType::Struct(st_name.clone())
            } else {
                bag.push(
                    Stage::TypeCheck,
                    file,
                    line,
                    format!("unknown struct '{}'", st_name),
                );
                VetoType::Int
            }
        }
        Expr::Index(arr, idx) => {
            let it = infer_expr(idx, file, line, scopes, structs, fn_sigs, bag);
            if it != VetoType::Int {
                bag.push(
                    Stage::TypeCheck,
                    file,
                    line,
                    "array index must be an integer",
                );
            }
            match scopes.lookup(arr) {
                Some(VetoType::Array(..)) => VetoType::Int,
                Some(other) => {
                    bag.push(
                        Stage::TypeCheck,
                        file,
                        line,
                        format!("'{}' has type {}, cannot index", arr, other.label()),
                    );
                    VetoType::Int
                }
                None => {
                    bag.push(
                        Stage::TypeCheck,
                        file,
                        line,
                        format!("undeclared array '{}'", arr),
                    );
                    VetoType::Int
                }
            }
        }
        Expr::UnaryNot(inner) => {
            let t = infer_expr(inner, file, line, scopes, structs, fn_sigs, bag);
            if t != VetoType::Int {
                bag.push(
                    Stage::TypeCheck,
                    file,
                    line,
                    "operator '!' can only be applied to integers",
                );
            }
            VetoType::Int
        }
        Expr::BinOp(l, op, r) => {
            let lt = infer_expr(l, file, line, scopes, structs, fn_sigs, bag);
            let rt = infer_expr(r, file, line, scopes, structs, fn_sigs, bag);
            if lt != rt || matches!(lt, VetoType::Struct(..) | VetoType::Array(..)) {
                bag.push(
                    Stage::TypeCheck,
                    file,
                    line,
                    format!(
                        "operator '{}' cannot be applied to {} and {}",
                        op,
                        lt.label(),
                        rt.label()
                    ),
                );
            }
            VetoType::Int
        }
        Expr::MethodCall(obj, method, args) => {
            let mangled = match scopes.lookup(obj) {
                Some(VetoType::Struct(st_name)) => format!("{}__{}", st_name, method),
                Some(VetoType::Enum(en_name)) => format!("{}__{}", en_name, method),
                Some(other) => {
                    bag.push(
                        Stage::TypeCheck,
                        file,
                        line,
                        format!(
                            "'{}' has type {}, cannot call method '{}'",
                            obj,
                            other.label(),
                            method
                        ),
                    );
                    return VetoType::Int;
                }
                None => {
                    bag.push(
                        Stage::TypeCheck,
                        file,
                        line,
                        format!("undeclared object '{}'", obj),
                    );
                    return VetoType::Int;
                }
            };
            if let Some((ptypes, rtype)) = fn_sigs.get(&mangled).cloned() {
                let mut full_args = vec![Expr::Var(obj.clone())];
                full_args.extend(args.iter().cloned());
                if full_args.len() != ptypes.len() {
                    bag.push(
                        Stage::TypeCheck,
                        file,
                        line,
                        format!(
                            "method '{}.{}' expects {} arguments, got {}",
                            obj,
                            method,
                            ptypes.len().saturating_sub(1),
                            args.len()
                        ),
                    );
                }
                for (arg, (expected, _)) in full_args.iter().zip(ptypes.iter()) {
                    let actual = infer_expr(arg, file, line, scopes, structs, fn_sigs, bag);
                    if actual != *expected {
                        bag.push(
                            Stage::TypeCheck,
                            file,
                            line,
                            format!(
                                "argument to method '{}.{}' expects {}, got {}",
                                obj,
                                method,
                                expected.label(),
                                actual.label()
                            ),
                        );
                    }
                }
                rtype
            } else {
                bag.push(
                    Stage::TypeCheck,
                    file,
                    line,
                    format!("unknown method '{}'", mangled),
                );
                VetoType::Int
            }
        }
        Expr::Call(name, args) => {
            if let Some((ptypes, rtype)) = fn_sigs.get(name).cloned() {
                if args.len() != ptypes.len() {
                    bag.push(
                        Stage::TypeCheck,
                        file,
                        line,
                        format!(
                            "function '{}' expects {} arguments, got {}",
                            name,
                            ptypes.len(),
                            args.len()
                        ),
                    );
                }
                let mut borrowed_muts: HashSet<String> = HashSet::new();
                for (arg, (expected, is_mut)) in args.iter().zip(ptypes.iter()) {
                    let actual = infer_expr(arg, file, line, scopes, structs, fn_sigs, bag);
                    if actual != *expected
                        && !matches!(
                            (&actual, expected),
                            (VetoType::Array(_), VetoType::Array(0))
                        )
                    {
                        bag.push(
                            Stage::TypeCheck,
                            file,
                            line,
                            format!(
                                "argument to function '{}' expects {}, got {}",
                                name,
                                expected.label(),
                                actual.label()
                            ),
                        );
                    }
                    if *is_mut {
                        if let Expr::Var(vname) = arg {
                            if !borrowed_muts.insert(vname.clone()) {
                                bag.push(
                                    Stage::TypeCheck,
                                    file,
                                    line,
                                    format!(
"exclusivity violation on '&mut': '{}' passed twice to '{}'",
vname, name
),
                                );
                            }
                        }
                    }
                }
                rtype
            } else {
                bag.push(
                    Stage::TypeCheck,
                    file,
                    line,
                    format!("call to unknown function '{}'", name),
                );
                VetoType::Int
            }
        }
    }
}
