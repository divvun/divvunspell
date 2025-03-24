(function() {
    var type_impls = Object.fromEntries([["divvunspell",[["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Transducer%3CF%3E-for-ThfstTransducer%3CI,+T,+F%3E\" class=\"impl\"><a class=\"src rightside\" href=\"src/divvunspell/transducer/thfst/mod.rs.html#88-238\">source</a><a href=\"#impl-Transducer%3CF%3E-for-ThfstTransducer%3CI,+T,+F%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;I, T, F&gt; <a class=\"trait\" href=\"divvunspell/transducer/trait.Transducer.html\" title=\"trait divvunspell::transducer::Transducer\">Transducer</a>&lt;F&gt; for <a class=\"struct\" href=\"divvunspell/transducer/thfst/struct.ThfstTransducer.html\" title=\"struct divvunspell::transducer::thfst::ThfstTransducer\">ThfstTransducer</a>&lt;I, T, F&gt;<div class=\"where\">where\n    I: <a class=\"trait\" href=\"divvunspell/transducer/trait.IndexTable.html\" title=\"trait divvunspell::transducer::IndexTable\">IndexTable</a>&lt;F&gt;,\n    T: <a class=\"trait\" href=\"divvunspell/transducer/trait.TransitionTable.html\" title=\"trait divvunspell::transducer::TransitionTable\">TransitionTable</a>&lt;F&gt;,\n    F: <a class=\"trait\" href=\"divvunspell/vfs/trait.File.html\" title=\"trait divvunspell::vfs::File\">File</a>,</div></h3></section></summary><div class=\"impl-items\"><details class=\"toggle\" open><summary><section id=\"associatedconstant.FILE_EXT\" class=\"associatedconstant trait-impl\"><a class=\"src rightside\" href=\"src/divvunspell/transducer/thfst/mod.rs.html#94\">source</a><a href=\"#associatedconstant.FILE_EXT\" class=\"anchor\">§</a><h4 class=\"code-header\">const <a href=\"divvunspell/transducer/trait.Transducer.html#associatedconstant.FILE_EXT\" class=\"constant\">FILE_EXT</a>: &amp;'static <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.str.html\">str</a> = &quot;thfst&quot;</h4></section></summary><div class='docblock'>file extension.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.from_path\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/divvunspell/transducer/thfst/mod.rs.html#96-120\">source</a><a href=\"#method.from_path\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"divvunspell/transducer/trait.Transducer.html#tymethod.from_path\" class=\"fn\">from_path</a>&lt;P, FS&gt;(fs: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.reference.html\">&amp;FS</a>, path: P) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.83.0/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;Self, <a class=\"enum\" href=\"divvunspell/transducer/enum.TransducerError.html\" title=\"enum divvunspell::transducer::TransducerError\">TransducerError</a>&gt;<div class=\"where\">where\n    P: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.83.0/core/convert/trait.AsRef.html\" title=\"trait core::convert::AsRef\">AsRef</a>&lt;<a class=\"struct\" href=\"https://doc.rust-lang.org/1.83.0/std/path/struct.Path.html\" title=\"struct std::path::Path\">Path</a>&gt;,\n    FS: <a class=\"trait\" href=\"divvunspell/vfs/trait.Filesystem.html\" title=\"trait divvunspell::vfs::Filesystem\">Filesystem</a>&lt;File = F&gt;,</div></h4></section></summary><div class='docblock'>read a transducer from a file.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.is_final\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/divvunspell/transducer/thfst/mod.rs.html#123-129\">source</a><a href=\"#method.is_final\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"divvunspell/transducer/trait.Transducer.html#tymethod.is_final\" class=\"fn\">is_final</a>(&amp;self, i: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.u32.html\">u32</a>) -&gt; <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.bool.html\">bool</a></h4></section></summary><div class='docblock'>check if given index is an end state.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.final_weight\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/divvunspell/transducer/thfst/mod.rs.html#132-138\">source</a><a href=\"#method.final_weight\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"divvunspell/transducer/trait.Transducer.html#tymethod.final_weight\" class=\"fn\">final_weight</a>(&amp;self, i: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.u32.html\">u32</a>) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.83.0/core/option/enum.Option.html\" title=\"enum core::option::Option\">Option</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.f32.html\">f32</a>&gt;</h4></section></summary><div class='docblock'>get end state weight of a state.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.has_transitions\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/divvunspell/transducer/thfst/mod.rs.html#141-158\">source</a><a href=\"#method.has_transitions\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"divvunspell/transducer/trait.Transducer.html#tymethod.has_transitions\" class=\"fn\">has_transitions</a>(&amp;self, i: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.u32.html\">u32</a>, s: <a class=\"enum\" href=\"https://doc.rust-lang.org/1.83.0/core/option/enum.Option.html\" title=\"enum core::option::Option\">Option</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.u16.html\">u16</a>&gt;) -&gt; <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.bool.html\">bool</a></h4></section></summary><div class='docblock'>check if there are transitions at given index.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.has_epsilons_or_flags\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/divvunspell/transducer/thfst/mod.rs.html#161-172\">source</a><a href=\"#method.has_epsilons_or_flags\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"divvunspell/transducer/trait.Transducer.html#tymethod.has_epsilons_or_flags\" class=\"fn\">has_epsilons_or_flags</a>(&amp;self, i: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.u32.html\">u32</a>) -&gt; <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.bool.html\">bool</a></h4></section></summary><div class='docblock'>check if there are free transitions at index.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.take_epsilons\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/divvunspell/transducer/thfst/mod.rs.html#175-181\">source</a><a href=\"#method.take_epsilons\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"divvunspell/transducer/trait.Transducer.html#tymethod.take_epsilons\" class=\"fn\">take_epsilons</a>(&amp;self, i: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.u32.html\">u32</a>) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.83.0/core/option/enum.Option.html\" title=\"enum core::option::Option\">Option</a>&lt;SymbolTransition&gt;</h4></section></summary><div class='docblock'>follow epsilon transitions.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.take_epsilons_and_flags\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/divvunspell/transducer/thfst/mod.rs.html#184-194\">source</a><a href=\"#method.take_epsilons_and_flags\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"divvunspell/transducer/trait.Transducer.html#tymethod.take_epsilons_and_flags\" class=\"fn\">take_epsilons_and_flags</a>(&amp;self, i: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.u32.html\">u32</a>) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.83.0/core/option/enum.Option.html\" title=\"enum core::option::Option\">Option</a>&lt;SymbolTransition&gt;</h4></section></summary><div class='docblock'>follow free transitions.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.take_non_epsilons\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/divvunspell/transducer/thfst/mod.rs.html#197-211\">source</a><a href=\"#method.take_non_epsilons\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"divvunspell/transducer/trait.Transducer.html#tymethod.take_non_epsilons\" class=\"fn\">take_non_epsilons</a>(&amp;self, i: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.u32.html\">u32</a>, symbol: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.u16.html\">u16</a>) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.83.0/core/option/enum.Option.html\" title=\"enum core::option::Option\">Option</a>&lt;SymbolTransition&gt;</h4></section></summary><div class='docblock'>follow transitions with given symbol.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.next\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/divvunspell/transducer/thfst/mod.rs.html#214-222\">source</a><a href=\"#method.next\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"divvunspell/transducer/trait.Transducer.html#tymethod.next\" class=\"fn\">next</a>(&amp;self, i: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.u32.html\">u32</a>, symbol: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.u16.html\">u16</a>) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.83.0/core/option/enum.Option.html\" title=\"enum core::option::Option\">Option</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.u32.html\">u32</a>&gt;</h4></section></summary><div class='docblock'>get next transition with a symbol.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.transition_input_symbol\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/divvunspell/transducer/thfst/mod.rs.html#225-227\">source</a><a href=\"#method.transition_input_symbol\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"divvunspell/transducer/trait.Transducer.html#tymethod.transition_input_symbol\" class=\"fn\">transition_input_symbol</a>(&amp;self, i: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.u32.html\">u32</a>) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.83.0/core/option/enum.Option.html\" title=\"enum core::option::Option\">Option</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.83.0/std/primitive.u16.html\">u16</a>&gt;</h4></section></summary><div class='docblock'>get input symbol number of given transition arc.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.alphabet\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/divvunspell/transducer/thfst/mod.rs.html#230-232\">source</a><a href=\"#method.alphabet\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"divvunspell/transducer/trait.Transducer.html#tymethod.alphabet\" class=\"fn\">alphabet</a>(&amp;self) -&gt; &amp;TransducerAlphabet</h4></section></summary><div class='docblock'>get transducer’s alphabet.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.mut_alphabet\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/divvunspell/transducer/thfst/mod.rs.html#235-237\">source</a><a href=\"#method.mut_alphabet\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"divvunspell/transducer/trait.Transducer.html#tymethod.mut_alphabet\" class=\"fn\">mut_alphabet</a>(&amp;mut self) -&gt; &amp;mut TransducerAlphabet</h4></section></summary><div class='docblock'>get transducer’s alphabet as mutable reference.</div></details></div></details>","Transducer<F>","divvunspell::transducer::thfst::MemmapThfstTransducer","divvunspell::transducer::thfst::FileThfstTransducer"]]]]);
    if (window.register_type_impls) {
        window.register_type_impls(type_impls);
    } else {
        window.pending_type_impls = type_impls;
    }
})()
//{"start":55,"fragment_lengths":[12245]}