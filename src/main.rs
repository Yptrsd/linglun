mod parser;

use parser::parse_and_extract;

fn main() {
    let score = r#"
        #tempo<120>
        #key<C>

        [1 2 3]
        {1 3 5}
        (1 2 3)
        0
        1^+1
        1^-1
        1^=1
        5.
        0.
        #dynamics<ff>
    "#;

    match parse_and_extract(score) {
        Ok(events) => {
            println!("✅ 解析成功！共 {} 个事件", events.len());
            println!("=");
            for (i, event) in events.iter().enumerate() {
                println!("  [{:2}] {}", i + 1, event);
            }
        }
        Err(e) => {
            println!("❌ {}", e);
        }
    }
}
