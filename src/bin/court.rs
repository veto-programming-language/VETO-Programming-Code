use vetoc::inquisitor::{Inquisitor, Verdict};

fn main() {
    let source = r#"
fn main() -> i32<0..255> {
    let speed: i32<0..100> = 40;
    let boost: i32<0..50> = 25;
    prove(speed + boost < 200);
    return speed + boost;
}
"#;
    let cond = std::env::args().nth(1).unwrap_or("speed + boost < 200".into());
    let mut inq = Inquisitor::new(&cond, source);
    match inq.auto_prove() {
        Verdict::Approve => println!("\n>>> одобрено"),
        Verdict::Reject => println!("\n>>> отклонено"),
    }
}
