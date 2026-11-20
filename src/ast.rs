pub type ExprList = Vec<Expr>;
pub type StmtList = Vec<Stmt>;
pub type ParamList = Vec<Param>;
pub type FnList = Vec<Function>;
pub type ConstList = Vec<ConstDef>;
pub type StructList = Vec<StructDef>;
pub type FieldDefList = Vec<FieldDef>;
pub type FieldInitList = Vec<(String, Expr)>;
pub type StrList = Vec<String>;

#[derive(Debug, Clone)]
pub enum Expr {
    Number(i64),
    Str(String),
    Var(String),
    Field(String, String),
    StructInit(String, FieldInitList),
    Index(String, Box<Expr>),
    Call(String, ExprList),
    UnaryNot(Box<Expr>),
    BinOp(Box<Expr>, String, Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        line: usize,
        name: String,
        type_name: String,
        min_val: i64,
        max_val: i64,
        value: Expr,
    },
    LetArray {
        line: usize,
        name: String,
        min_val: i64,
        max_val: i64,
        len: usize,
        elements: ExprList,
    },
    Assign {
        line: usize,
        name: String,
        value: Expr,
    },
    FieldAssign {
        line: usize,
        obj: String,
        field: String,
        value: Expr,
    },
    IndexAssign {
        line: usize,
        name: String,
        index: Expr,
        value: Expr,
    },
    Loop {
        line: usize,
        body: StmtList,
    },
    While {
        line: usize,
        cond: Expr,
        body: StmtList,
    },
    For {
        line: usize,
        var: String,
        start: Expr,
        end: Expr,
        body: StmtList,
    },
    Break {
        line: usize,
    },
    Continue {
        line: usize,
    },
    If {
        line: usize,
        cond: Expr,
        then_body: StmtList,
        else_body: StmtList,
    },
    ExprStmt {
        line: usize,
        expr: Expr,
    },
    Prove {
        line: usize,
        cond: String,
    },
    Return {
        line: usize,
        expr: Expr,
    },
}

#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: String,
    pub min_val: i64,
    pub max_val: i64,
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub line: usize,
    pub name: String,
    pub fields: FieldDefList,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub type_name: String,
    pub min_val: i64,
    pub max_val: i64,
}

#[derive(Debug, Clone)]
pub struct ConstDef {
    pub line: usize,
    pub name: String,
    pub min_val: i64,
    pub max_val: i64,
    pub value: Expr,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub line: usize,
    pub name: String,
    pub params: ParamList,
    pub ret_type: String,
    pub ret_min: i64,
    pub ret_max: i64,
    pub body: StmtList,
}

#[derive(Debug, Default)]
pub struct Program {
    pub imports: StrList,
    pub structs: StructList,
    pub consts: ConstList,
    pub functions: FnList,
}
