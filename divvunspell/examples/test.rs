extern crate divvunspell;

use std::sync::Arc;
use std::{path::Path, time::Instant};

use divvunspell::archive::{SpellerArchive, ZipSpellerArchive};
use divvunspell::speller::{Speller, SpellerConfig};
use divvunspell::transducer::hfst::HfstTransducer;

fn run(speller: Arc<dyn Speller + Send + Sync>, line: &TestLine, cfg: &SpellerConfig) {
    let _ = speller.suggest_with_config(line.0, &cfg);
}

fn time_suggest(
    speller: Arc<dyn Speller + Send + Sync>,
    line: &TestLine,
    cfg: &SpellerConfig,
) -> String {
    // println!("[!] Test: {}; Expected: {}; Orig. time: {}; Orig. results:\n    {}", line.0, line.1, line.2, line.3.join(", "));

    let now = Instant::now();
    let res = speller.suggest_with_config(line.0, &cfg);
    let then = now.elapsed();

    let len = if res.len() >= 10 { 10 } else { res.len() };

    let words: Vec<&str> = res[0..len].iter().map(|x| x.value()).collect();
    let _out: Vec<String> = res[0..len]
        .iter()
        .map(|x| format!("    {:>10.6}  {}", x.weight(), x.value()))
        .collect();

    // println!("[>] Actual time: {}.{}; Has expected: {}; Results: {}\n{}\n",
    //         then.as_secs(), then.subsec_nanos() / 1000000, words.contains(&line.1), words.len(), &out.join("\n"));

    format!(
        "{} -> {}:\n[>] {}, {} -> {}.{}",
        line.0,
        line.1,
        words.contains(&line.1),
        line.2,
        then.as_secs(),
        then.subsec_nanos() / 1000
    )
}

type TestLine = (&'static str, &'static str, f32, &'static [&'static str]);

static tuples: &'static [TestLine] = &[
    // ("gáibiđivččii", "gáibidivččii", 8.012096, &["gálbirii"]),
    // ("gárvanivččii", "gárvánivččii", 7.510769, &["árranii", "gálganii", "gávvalii", "čárvagii", "šávanii", "gášanii", "gávvasii", "gákkanii", "gávažii"]),
    // ("nannesivččii", "nannešii", 7.329407, &["nannemii", "nannosii", "nannámii", "naneessii", "naniásii", "nanedesii", "naneásii", "nanitesii", "naniessii", "nanidesii"]),
    // ("eanavuoigatvuohtadutkamušas", "eanavuoigatvuođadutkamušas", 7.185912, &["eanavuoigatvuođadutkamušas", "eananvuoigatvuođadutkamušas", "eanavuoigatvuođadutkamuša", "eanavuoigatvuođadutkamušat", "leanavuoigatvuođadutkamušas", "eanavuoigatvuođadutkamušase", "eanavuoigatvuođadutkamušasi", "eanavuoigatvuođahutkamušas", "eanavuoigatvuođadutkamušbas", "beanavuoigatvuođadutkamušas"]),
    (
        "vuovdinfállovuogiŧ",
        "vuovdinfállovuogit",
        7.000394,
        &[
            "vuovdinfállovuogi",
            "vuovdinfállovuogis",
            "vuovdinfállovuogit",
            "vuovdinfállovuoge",
            "vuovdinfállovuogigo",
            "vuovdinfállovuogiba",
            "vuovdinfállovuogibe",
            "vuovdinfállovuogige",
            "vuovdinfállovuogát",
            "vuovdinbállovuogi",
        ],
    ),
    (
        "guollebiebmandoalliide",
        "guollebiebmandolliide",
        6.93927,
        &[
            "guollebiebmandoalliiđa",
            "guollebiebmandoalliviđe",
            "guollebiebmanduvlliide",
            "guollebiebmandoalliáidde",
            "guollebiebmandolliide",
            "guollebiebmandoalliidea",
            "guollebiebmandoalláde",
            "guollebiebmandolliid",
            "guollebiebmandoalliideat",
            "guollebiebmandoalliideii",
        ],
    ),
    (
        "guolledikšunbivdimiš",
        "guolledikšunbivdimis",
        6.935191,
        &[
            "guolledikšunbivdimii",
            "guolledikšunbivdimin",
            "guolledikšunbivdimis",
            "guolledikšunbivdimuš",
            "guoledikšunbivdimii",
            "guolledikšunbivdimiid",
            "guolledikšunbivdimiin",
            "golledikšunbivdimin",
            "guollefikšunbivdimis",
            "guolbedikšunbivdimii",
        ],
    ),
    (
        "oaidnitálgogeasis",
        "oaidnit álgogeasis",
        6.913222,
        &[
            "oaidnigálgugeasis",
            "oaidnidálgageasis",
            "oaidnidállogeasis",
            "oaidnidávgogeasis",
            "oaidnidáigogeasis",
            "oaidnidálgaogeasis",
            "oaidniálgogeasis",
            "oaidnibálggogeasis",
            "oaidnidáluogeasis",
            "oaidnilálgogeasis",
        ],
    ),
    (
        "arkitektuvragilvvohallama",
        "arkitekturgilvvohallama",
        6.898086,
        &[
            "arkitekturgilvvuhallama",
            "arkitekturgilvvohállama",
            "arkitekturgilvvohallama",
            "arkitektuvra-gilvvuhallama",
            "arkitekturgilvvohallamat",
            "arkitektuvra-gilvvohállama",
            "arkitektuvra-gilvvohallama",
            "arkitekturgilvvohallaman",
            "arkitekturgilvvoballama",
            "arkitekturgilvvoheallama",
        ],
    ),
    (
        "borramušráhkadanlihtiiid",
        "borramušráhkadanlihtiid",
        6.78258,
        &[
            "borramušráhkadanlihtiid",
            "borramušráhkadanlihtti-id",
            "borramušráhkadanihtimiid",
            "borramušráhkadanlihtoriid",
            "borramušráhkadanlihtuid",
            "borramušráhkadanlihpiid",
            "borramušráhkadanlihtariid",
            "borramušráhkadanláhttiid",
            "borramušráhkadandihttiid",
            "borramušráhkadanfihtiid",
        ],
    ),
    ("humašivččii", "humašii", 6.762947, &["gumažii", "jumažii"]),
    (
        "buorranivččii",
        "buorránivččii",
        6.731601,
        &["boaranii", "boarragii", "borramii", "borrasii", "boranii"],
    ),
    (
        "ovdaskuvlaoahpaheaddjiide",
        "ovdaskuvlaoahpaheddjiide",
        6.705206,
        &[
            "ovdaskuvlaoahpaheddjiide",
            "ovdaskuvllaoahpaheddjiide",
            "ovddaskuvlaoahpaheddjiide",
            "ovdaskuvlaoahppaheaddjuide",
            "ovdaskuvlaoahpaheaddjáde",
            "ovdaskuvlaoahpaheddjiid",
            "ordaskuvlaoahpaheddjiide",
            "ovdoskuvlaoahpaheddjiide",
            "ovdaskuvlaoahpaheaddjeidea",
            "ovdaskuvlaoahpaheaddjige",
        ],
    ),
    (
        "ovdanbuktinvuogivuostedeaddun",
        "ovdanbuktinvuogi vuostedeaddun",
        6.648509,
        &[
            "ovdanbuktinsoagivuostedeaddun",
            "ovdánbuktinvuoigivuostedeaddun",
            "ovdánbuktinvuolgivuostedeaddun",
            "ovdánbuktinvuovgivuostedeaddun",
            "ovdanbuktinvuolgiijavuostedeaddun",
            "ovdanbuktinvuoigivuostedeaddun",
            "ovdanbuktinvuolgivuostedeaddun",
            "ovdanbuktinvuoigiijavuostedeaddun",
            "ovdanbuktinvuovgiijavuostedeaddun",
            "ovdanbuktinvuovgivuostedeaddun",
        ],
    ),
    (
        "oastinovdavuoigatvuodas",
        "oastinovdavuoigatvuođas",
        6.636999,
        &[
            "oastinovdavuoigatvuođas",
            "oastinovdavuoigatvuođa",
            "oastinovdavuoigatvuođat",
            "oastilovdavuoigatvuođas",
            "oastiovdavuoigatvuođas",
            "eastinovdavuoigatvuođas",
            "oastinovdovuoigatvuođas",
            "goastinovdavuoigatvuođas",
            "noastinovdavuoigatvuođas",
            "oastinordavuoigatvuođas",
        ],
    ),
    ("lagidivččii", "lágidivččii", 6.526697, &["eamidii"]),
    (
        "sámekultuvrapolitihkkalaš",
        "sámekulturpolitihkalaš",
        6.496088,
        &["sámekulturpolitihkalaš", "sámekulturpolitihkkaraš"],
    ),
    (
        "hálidivččii",
        "háliidivččii",
        6.484202,
        &[
            "báldii",
            "báládii",
            "holdii",
            "hálbmii",
            "álisii",
            "hávdii",
            "hálkii",
            "hállii",
            "háikii",
            "háipii",
        ],
    ),
    (
        "johttivuovdinbuvriid",
        "johttivuovdinbuvrriid",
        6.46664,
        &[
            "johttivuovdinbuvrriid",
            "johttiijavuovdinbuvrriid",
            "johttivuovdinboriid",
            "johttivuovdinborriid",
            "johttivuovdinbovrii",
            "johttivuovdinbuvrrit",
            "johttivuovdinguvrriid",
            "johttivuovdinsuvrriid",
            "johttivuovdibuvrriid",
            "johttivuordinbuvrriid",
        ],
    ),
];

fn main() {
    // use divvunspell::COUNTER;
    // use std::fs::File;
    // use std::io::BufReader;
    // use std::io::prelude::*;

    // let acceptor = File::open("./se/acceptor.default.hfst").unwrap();
    // let mut acceptor_buf = &[];
    // let _ = BufReader::new(acceptor).read_to_end(&mut acceptor_buf);

    // let errmodel = File::open("./se/errmodel.default.hfst").unwrap();
    // let mut errmodel_buf = &[];
    // let _ = BufReader::new(errmodel).read_to_end(&mut errmodel_buf);

    // let lexicon = Transducer::from_bytes(&acceptor_buf);
    // let mutator = Transducer::from_bytes(&errmodel_buf);

    // let speller = Speller::new(mutator, lexicon);

    // let now = Instant::now();
    // let speller = unaligned.speller();
    // let res = speller.suggest("nuvviDspeller");
    // let ver: Vec<&str> = res.iter().filter(|x| x.weight() <= 3.0).map(|x| x.value()).collect();
    // println!("{:?}", now.elapsed());

    // let now = Instant::now();
    // let speller = aligned.speller();
    // let res = speller.suggest("nuvviDspeller");
    // let ver: Vec<&str> = res.iter().filter(|x| x.weight() <= 3.0).map(|x| x.value()).collect();
    // println!("{:?}", now.elapsed());

    // let human_rights = ["buot", "olbmot", "leat", "riegádan", "friddjan", "ja",
    //     "olmmošárvvu", "ja", "olmmošvuoigatvuođaid", "dáfus", "Sii", "leat",
    //     "jierbmalaš", "olbmot", "geain", "lea", "oamedovdu", "ja", "sii", "gálggaše",
    //     "leat", "dego", "vieljačagat"];

    // let correct: Vec<bool> = human_rights.iter().map(|w| speller.is_correct(w)).collect();
    let mut cfg = SpellerConfig::default();

    // let res: Vec<Vec<Suggestion>> = human_rights.iter().map(|w| speller.suggest(w, &cfg)).collect();

    // println!("{:?}", correct);
    // println!("{:?}", res);

    // let res = speller.suggest("gáibiđivččii", &cfg);
    // println!("{:?}", ver);

    // let words = ["vuovdinfállovuogiŧ", "eanavuoigatvuohtadutkamušas", "nannesivččii", "gárvanivččii", "gáibiđivččii"];

    let unaligned = ZipSpellerArchive::open(Path::new("./unaligned-test.zhfst")).unwrap();
    // let unaligned = ChfstBundle::from_path(&std::path::Path::new("./out.chfst")).unwrap();
    // let res = unaligned.speller().suggest_with_config("same", &cfg);
    // let aligned = SpellerArchive::open("./aligned-test.zhfst").unwrap();
    let speller = unaligned.speller();

    for i in 0..1 {
        //14..=20 {
        let now = Instant::now();
        for line in tuples.iter() {
            // let mut ncfg = cfg.clone();
            // cfg.seen_node_sample_rate = i;
            cfg.node_pool_size = 128;
            // println!("{}",
            run(Arc::clone(&speller), &line, &cfg);
            // );
            break;
        }
        let then = now.elapsed();
        println!(
            "{} ({}): {}.{}",
            i,
            i,
            then.as_secs(),
            then.subsec_nanos() / 1000
        );
    }

    // std::thread::sleep(std::time::Duration::from_millis(10000));
    // let unaligned_time = now.elapsed();

    // let now = Instant::now();
    // for line in tuples.iter() {
    //     time_suggest(aligned.speller(), &line);
    // }
    // let aligned_time = now.elapsed();

    // println!("Unaligned: {:?}", unaligned_time);
    // println!("Aligned: {:?}", aligned_time);

    // println!("{:?}", *COUNTER.lock().unwrap());

    //speller.suggest("vuovdinfállovuogiŧ");
}
