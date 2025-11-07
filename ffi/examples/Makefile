# Makefile for divvunspell FFI examples

# Detect OS
UNAME_S := $(shell uname -s)

# Library name
ifeq ($(UNAME_S),Darwin)
    LIB_EXT = dylib
    RPATH_FLAG = -Wl,-rpath,$(CURDIR)/../../target/debug
else ifeq ($(UNAME_S),Linux)
    LIB_EXT = so
    RPATH_FLAG = -Wl,-rpath,$(CURDIR)/../../target/debug
else
    LIB_EXT = dll
    RPATH_FLAG =
endif

# Paths
LIB_DIR = ../../target/debug
LIB_NAME = divvunspell
INCLUDE_DIR = ../include

# Compiler flags
CC = cc
CFLAGS = -Wall -I$(INCLUDE_DIR)
LDFLAGS = $(LIB_DIR)/lib$(LIB_NAME).$(LIB_EXT) $(RPATH_FLAG)

# Targets
EXAMPLES = tokenize spellcheck

.PHONY: all clean build-lib

all: build-lib $(EXAMPLES)

build-lib:
	@echo "Building FFI library..."
	@cd ../.. && cargo build -p divvunspell-ffi

%: %.c
	@echo "Compiling $@..."
	$(CC) $(CFLAGS) -o $@ $< $(LDFLAGS)

clean:
	rm -f $(EXAMPLES) test_*

run-tokenize: tokenize
	@echo "Running tokenize example..."
	./tokenize

.DEFAULT_GOAL := all
