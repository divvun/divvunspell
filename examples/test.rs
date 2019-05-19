extern crate divvunspell;

use std::time::{Duration, Instant};
use std::sync::Arc;

use divvunspell::archive::SpellerArchive;
use divvunspell::speller::{Speller, SpellerConfig};
use divvunspell::speller::suggestion::Suggestion;
use divvunspell::transducer::{HfstTransducer};

fn time_suggest(speller: Arc<Speller<HfstTransducer>>, line: &TestLine, cfg: SpellerConfig) -> String {
    
    // println!("[!] Test: {}; Expected: {}; Orig. time: {}; Orig. results:\n    {}", line.0, line.1, line.2, line.3.join(", "));

    let now = Instant::now();
    let res = speller.suggest_with_config(line.0, &cfg);
    let then = now.elapsed();

    let len = if res.len() >= 10 { 10 } else { res.len() };

    let words: Vec<&str> = res[0..len].iter().map(|x| x.value()).collect();
    let _out: Vec<String> = res[0..len].iter().map(|x| format!("    {:>10.6}  {}", x.weight(), x.value())).collect();

    // println!("[>] Actual time: {}.{}; Has expected: {}; Results: {}\n{}\n", 
    //         then.as_secs(), then.subsec_nanos() / 1000000, words.contains(&line.1), words.len(), &out.join("\n"));

    format!("{} -> {}:\n[>] {}, {} -> {}.{}", line.0, line.1, words.contains(&line.1), line.2, then.as_secs(), then.subsec_nanos() / 1000)
}

type TestLine = (&'static str, &'static str, f32, Vec<&'static str>);

fn main() {
    // use divvunspell::COUNTER;
    // use std::fs::File;
    // use std::io::BufReader;
    // use std::io::prelude::*;

    // let acceptor = File::open("./se/acceptor.default.hfst").unwrap();
    // let mut acceptor_buf = vec![];
    // let _ = BufReader::new(acceptor).read_to_end(&mut acceptor_buf);

    // let errmodel = File::open("./se/errmodel.default.hfst").unwrap();
    // let mut errmodel_buf = vec![];
    // let _ = BufReader::new(errmodel).read_to_end(&mut errmodel_buf);

    // let lexicon = Transducer::from_bytes(&acceptor_buf);
    // let mutator = Transducer::from_bytes(&errmodel_buf);

    // let speller = Speller::new(mutator, lexicon);

    let tuples: &[TestLine] = &[
        // ("gáibiđivččii", "gáibidivččii", 8.012096, vec!["gálbirii"]),
        // ("gárvanivččii", "gárvánivččii", 7.510769, vec!["árranii", "gálganii", "gávvalii", "čárvagii", "šávanii", "gášanii", "gávvasii", "gákkanii", "gávažii"]),
        // ("nannesivččii", "nannešii", 7.329407, vec!["nannemii", "nannosii", "nannámii", "naneessii", "naniásii", "nanedesii", "naneásii", "nanitesii", "naniessii", "nanidesii"]),
        // ("eanavuoigatvuohtadutkamušas", "eanavuoigatvuođadutkamušas", 7.185912, vec!["eanavuoigatvuođadutkamušas", "eananvuoigatvuođadutkamušas", "eanavuoigatvuođadutkamuša", "eanavuoigatvuođadutkamušat", "leanavuoigatvuođadutkamušas", "eanavuoigatvuođadutkamušase", "eanavuoigatvuođadutkamušasi", "eanavuoigatvuođahutkamušas", "eanavuoigatvuođadutkamušbas", "beanavuoigatvuođadutkamušas"]),
        ("vuovdinfállovuogiŧ", "vuovdinfállovuogit", 7.000394, vec!["vuovdinfállovuogi", "vuovdinfállovuogis", "vuovdinfállovuogit", "vuovdinfállovuoge", "vuovdinfállovuogigo", "vuovdinfállovuogiba", "vuovdinfállovuogibe", "vuovdinfállovuogige", "vuovdinfállovuogát", "vuovdinbállovuogi"]),
        ("guollebiebmandoalliide", "guollebiebmandolliide", 6.93927, vec!["guollebiebmandoalliiđa", "guollebiebmandoalliviđe", "guollebiebmanduvlliide", "guollebiebmandoalliáidde", "guollebiebmandolliide", "guollebiebmandoalliidea", "guollebiebmandoalláde", "guollebiebmandolliid", "guollebiebmandoalliideat", "guollebiebmandoalliideii"]),
        ("guolledikšunbivdimiš", "guolledikšunbivdimis", 6.935191, vec!["guolledikšunbivdimii", "guolledikšunbivdimin", "guolledikšunbivdimis", "guolledikšunbivdimuš", "guoledikšunbivdimii", "guolledikšunbivdimiid", "guolledikšunbivdimiin", "golledikšunbivdimin", "guollefikšunbivdimis", "guolbedikšunbivdimii"]),
        ("oaidnitálgogeasis", "oaidnit álgogeasis", 6.913222, vec!["oaidnigálgugeasis", "oaidnidálgageasis", "oaidnidállogeasis", "oaidnidávgogeasis", "oaidnidáigogeasis", "oaidnidálgaogeasis", "oaidniálgogeasis", "oaidnibálggogeasis", "oaidnidáluogeasis", "oaidnilálgogeasis"]),
        ("arkitektuvragilvvohallama", "arkitekturgilvvohallama", 6.898086, vec!["arkitekturgilvvuhallama", "arkitekturgilvvohállama", "arkitekturgilvvohallama", "arkitektuvra-gilvvuhallama", "arkitekturgilvvohallamat", "arkitektuvra-gilvvohállama", "arkitektuvra-gilvvohallama", "arkitekturgilvvohallaman", "arkitekturgilvvoballama", "arkitekturgilvvoheallama"]),
        ("borramušráhkadanlihtiiid", "borramušráhkadanlihtiid", 6.78258, vec!["borramušráhkadanlihtiid", "borramušráhkadanlihtti-id", "borramušráhkadanihtimiid", "borramušráhkadanlihtoriid", "borramušráhkadanlihtuid", "borramušráhkadanlihpiid", "borramušráhkadanlihtariid", "borramušráhkadanláhttiid", "borramušráhkadandihttiid", "borramušráhkadanfihtiid"]),
        ("humašivččii", "humašii", 6.762947, vec!["gumažii", "jumažii"]),
        ("buorranivččii", "buorránivččii", 6.731601, vec!["boaranii", "boarragii", "borramii", "borrasii", "boranii"]),
        ("ovdaskuvlaoahpaheaddjiide", "ovdaskuvlaoahpaheddjiide", 6.705206, vec!["ovdaskuvlaoahpaheddjiide", "ovdaskuvllaoahpaheddjiide", "ovddaskuvlaoahpaheddjiide", "ovdaskuvlaoahppaheaddjuide", "ovdaskuvlaoahpaheaddjáde", "ovdaskuvlaoahpaheddjiid", "ordaskuvlaoahpaheddjiide", "ovdoskuvlaoahpaheddjiide", "ovdaskuvlaoahpaheaddjeidea", "ovdaskuvlaoahpaheaddjige"]),
        ("ovdanbuktinvuogivuostedeaddun", "ovdanbuktinvuogi vuostedeaddun", 6.648509, vec!["ovdanbuktinsoagivuostedeaddun", "ovdánbuktinvuoigivuostedeaddun", "ovdánbuktinvuolgivuostedeaddun", "ovdánbuktinvuovgivuostedeaddun", "ovdanbuktinvuolgiijavuostedeaddun", "ovdanbuktinvuoigivuostedeaddun", "ovdanbuktinvuolgivuostedeaddun", "ovdanbuktinvuoigiijavuostedeaddun", "ovdanbuktinvuovgiijavuostedeaddun", "ovdanbuktinvuovgivuostedeaddun"]),
        ("oastinovdavuoigatvuodas", "oastinovdavuoigatvuođas", 6.636999, vec!["oastinovdavuoigatvuođas", "oastinovdavuoigatvuođa", "oastinovdavuoigatvuođat", "oastilovdavuoigatvuođas", "oastiovdavuoigatvuođas", "eastinovdavuoigatvuođas", "oastinovdovuoigatvuođas", "goastinovdavuoigatvuođas", "noastinovdavuoigatvuođas", "oastinordavuoigatvuođas"]),
        ("lagidivččii", "lágidivččii", 6.526697, vec!["eamidii"]),
        ("sámekultuvrapolitihkkalaš", "sámekulturpolitihkalaš", 6.496088, vec!["sámekulturpolitihkalaš", "sámekulturpolitihkkaraš"]),
        ("hálidivččii", "háliidivččii", 6.484202, vec!["báldii", "báládii", "holdii", "hálbmii", "álisii", "hávdii", "hálkii", "hállii", "háikii", "háipii"]),
        ("johttivuovdinbuvriid", "johttivuovdinbuvrriid", 6.46664, vec!["johttivuovdinbuvrriid", "johttiijavuovdinbuvrriid", "johttivuovdinboriid", "johttivuovdinborriid", "johttivuovdinbovrii", "johttivuovdinbuvrrit", "johttivuovdinguvrriid", "johttivuovdinsuvrriid", "johttivuovdibuvrriid", "johttivuordinbuvrriid"]),
    ];

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
    let cfg = SpellerConfig {
        max_weight: Some(100.0),
        n_best: Some(5),
        beam: None,
        pool_max: 128,
        pool_start: 128,
        seen_node_sample_rate: 20,
        with_caps: true
    };

    // let res: Vec<Vec<Suggestion>> = human_rights.iter().map(|w| speller.suggest(w, &cfg)).collect();

    // println!("{:?}", correct);
    // println!("{:?}", res);

    // let res = speller.suggest("gáibiđivččii", &cfg);
    // println!("{:?}", ver);

    // let words = ["vuovdinfállovuogiŧ", "eanavuoigatvuohtadutkamušas", "nannesivččii", "gárvanivččii", "gáibiđivččii"];

    let unaligned = SpellerArchive::new("./unaligned-test.zhfst").unwrap();
    // let res = unaligned.speller().suggest_with_config("same", &cfg);
    // let aligned = SpellerArchive::new("./aligned-test.zhfst").unwrap();

    let now = Instant::now();
    for i in 0..50 {
        let now = Instant::now();
        for line in tuples.iter() {
            let mut ncfg = cfg.clone();
            ncfg.seen_node_sample_rate = i;
            time_suggest(unaligned.speller(), &line, ncfg);
        }
        let then = now.elapsed();
        println!("{}: {}.{}", i, then.as_secs(), then.subsec_nanos() / 1000);
    }
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
