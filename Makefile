ifdef CARGO_FEATURES
FLAGS += --features $(CARGO_FEATURES)
endif

ifdef CARGO_BIN
FLAGS += --bin $(CARGO_BIN)
else
FLAGS += --lib
endif

ifeq "$(CONFIGURATION)" "Release"
FLAGS += --release
endif

xcode:
	$(CARGO_HOME)/bin/cargo build $(FLAGS)
	rm -f target/$(CONFIGURATION)/libdivvunspell.dylib
xcodeinstall:
	$(CARGO_HOME)/bin/cargo build $(FLAGS)
	rm -f target/$(CONFIGURATION)/libdivvunspell.dylib
xcodeclean:
	$(CARGO_HOME)/bin/cargo clean
xcodelipo:
	$(CARGO_HOME)/bin/cargo lipo --targets aarch64-apple-ios,x86_64-apple-ios,armv7-apple-ios $(FLAGS)
xcodelipoinstall:
	$(CARGO_HOME)/bin/cargo lipo --targets aarch64-apple-ios,x86_64-apple-ios,armv7-apple-ios $(FLAGS)
xcodelipoclean:
	$(CARGO_HOME)/bin/cargo clean
