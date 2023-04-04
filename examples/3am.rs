use chrono::Timelike;

fn main() {
    let (pm, hour) = chrono::Utc::now().hour12();
    dbg!(pm, hour);
    if 3 == hour && false == pm {
        println!("The painted cow !");
    }
}
