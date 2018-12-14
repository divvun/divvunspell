ifdef CARGO_FEATURES
FLAGS += --features $(CARGO_FEATURES)
endif

ifdef CARGO_BIN
FLAGS += --bin $(CARGO_BIN)
endif

ifeq "$(CONFIGURATION)" "Release"
FLAGS += --release
endif

xcode:
	$(CARGO_HOME)/bin/cargo build $(FLAGS)
	rm -f target/$(CONFIGURATION)/libhfstospell.dylib
xcodeinstall:
	$(CARGO_HOME)/bin/cargo build $(FLAGS)
	rm -f target/$(CONFIGURATION)/libhfstospell.dylib
xcodeclean:
	$(CARGO_HOME)/bin/cargo clean
xcodelipo:
	$(CARGO_HOME)/bin/cargo lipo $(FLAGS)
xcodelipoinstall:
	$(CARGO_HOME)/bin/cargo lipo $(FLAGS)
xcodelipoclean:
	$(CARGO_HOME)/bin/cargo clean
