ifdef CARGO_FEATURES
FLAGS += --features $(CARGO_FEATURES)
endif

ifdef CARGO_BIN
FLAGS += --bin $(CARGO_BIN)
endif

#ifeq "$(CONFIGURATION)" "Release"
#FLAGS += --release
#endif

xcode:
	$(CARGO_HOME)/bin/cargo build $(FLAGS)
	rm -f target/$(CONFIGURATION)/libdivvunspell.dylib
xcodeinstall:
	$(CARGO_HOME)/bin/cargo build $(FLAGS)
	rm -f target/$(CONFIGURATION)/libdivvunspell.dylib
xcodeclean:
	$(CARGO_HOME)/bin/cargo clean
xcodelipo:
	$(CARGO_HOME)/bin/cargo lipo --xcode-integ $(FLAGS)
xcodelipoinstall:
	$(CARGO_HOME)/bin/cargo lipo --xcode-integ $(FLAGS)
xcodelipoclean:
	$(CARGO_HOME)/bin/cargo clean
