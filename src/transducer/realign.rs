use crate::transducer::Transducer;
use std::fs::File;
use std::io::{Read, Write, Seek, BufReader};

fn realign(transducer: &Transducer<'_>) {
    let header_len = transducer.header().len() + transducer.alphabet().len();
    println!("Header length: {}", header_len);

    let header_len_diff = header_len % 4;

    let mut file = File::create("./test-align.hfst").unwrap();
    let buf = transducer.buffer();
    file.write(&buf[0..header_len]).unwrap();

    if header_len_diff != 0 {
        let c = 4 - header_len_diff;
        println!("Header length is not divisible by 4; must be corrected by {}", c);

        for _ in 0..c {
            file.write(b"\0").unwrap();
        }
    }
    
    file.write(&buf[header_len..buf.len()]).unwrap();
}

#[test]
fn test_realign() {
    // let acceptor = File::open("./se/acceptor.default.hfst").unwrap();
    let acceptor = File::open("./test-align.hfst").unwrap();
    let mut acceptor_buf = vec![];
    let _ = BufReader::new(acceptor).read_to_end(&mut acceptor_buf);
    let transducer = Transducer::from_bytes(&acceptor_buf);
    realign(&transducer);
}