ifdef CARGO_FEATURES
FLAGS += --features $(CARGO_FEATURES)
endif

# Always want a release build for Divvunspell
FLAGS += --release

xcode:
	$(CARGO_HOME)/bin/cargo build $(FLAGS)
	rm -f target/$(CONFIGURATION)/libdivvunspell.dylib
xcodeinstall:
	$(CARGO_HOME)/bin/cargo build $(FLAGS)
	rm -f target/$(CONFIGURATION)/libdivvunspell.dylib
xcodeclean:
	$(CARGO_HOME)/bin/cargo clean
xcodelipo:
	$(CARGO_HOME)/bin/cargo lipo $(FLAGS)
xcodelipoinstall:
	$(CARGO_HOME)/bin/cargo lipo $(FLAGS)
xcodelipoclean:
	$(CARGO_HOME)/bin/cargo clean
