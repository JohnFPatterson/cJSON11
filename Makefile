CJSON_OBJ = cJSON.o
UTILS_OBJ = cJSON_Utils.o
CJSON_LIBNAME = libcjson
UTILS_LIBNAME = libcjson_utils
CJSON_TEST = cJSON_test

CJSON_TEST_SRC = cJSON.c test.c

LDLIBS = -lm

LIBVERSION = 1.7.19
CJSON_SOVERSION = 1
UTILS_SOVERSION = 1

CJSON_SO_LDFLAG=-Wl,-soname=$(CJSON_LIBNAME).so.$(CJSON_SOVERSION)
UTILS_SO_LDFLAG=-Wl,-soname=$(UTILS_LIBNAME).so.$(UTILS_SOVERSION)

PREFIX ?= /usr/local
INCLUDE_PATH ?= include/cjson
LIBRARY_PATH ?= lib

INSTALL_INCLUDE_PATH = $(DESTDIR)$(PREFIX)/$(INCLUDE_PATH)
INSTALL_LIBRARY_PATH = $(DESTDIR)$(PREFIX)/$(LIBRARY_PATH)

INSTALL ?= cp -a

CC = gcc -std=c89

# validate gcc version for use fstack-protector-strong
MIN_GCC_VERSION = "4.9"
GCC_VERSION := "`$(CC) -dumpversion`"
IS_GCC_ABOVE_MIN_VERSION := $(shell expr "$(GCC_VERSION)" ">=" "$(MIN_GCC_VERSION)")
ifeq "$(IS_GCC_ABOVE_MIN_VERSION)" "1"
    CFLAGS += -fstack-protector-strong
else
    CFLAGS += -fstack-protector
endif

PIC_FLAGS = -fPIC
R_CFLAGS = $(PIC_FLAGS) -pedantic -Wall -Werror -Wstrict-prototypes -Wwrite-strings -Wshadow -Winit-self -Wcast-align -Wformat=2 -Wmissing-prototypes -Wstrict-overflow=2 -Wcast-qual -Wc++-compat -Wundef -Wswitch-default -Wconversion $(CFLAGS)

uname := $(shell sh -c 'uname -s 2>/dev/null || echo false')

#library file extensions
SHARED = so
STATIC = a

## create dynamic (shared) library on Darwin (base OS for MacOSX and IOS)
ifeq (Darwin, $(uname))
	SHARED = dylib
	CJSON_SO_LDFLAG = ""
	UTILS_SO_LDFLAG = ""
endif

#cJSON library names
CJSON_SHARED = $(CJSON_LIBNAME).$(SHARED)
CJSON_SHARED_VERSION = $(CJSON_LIBNAME).$(SHARED).$(LIBVERSION)
CJSON_SHARED_SO = $(CJSON_LIBNAME).$(SHARED).$(CJSON_SOVERSION)
CJSON_STATIC = $(CJSON_LIBNAME).$(STATIC)

#cJSON_Utils library names
UTILS_SHARED = $(UTILS_LIBNAME).$(SHARED)
UTILS_SHARED_VERSION = $(UTILS_LIBNAME).$(SHARED).$(LIBVERSION)
UTILS_SHARED_SO = $(UTILS_LIBNAME).$(SHARED).$(UTILS_SOVERSION)
UTILS_STATIC = $(UTILS_LIBNAME).$(STATIC)

SHARED_CMD = $(CC) -shared -o

.PHONY: all shared static tests clean install

all: shared static tests

shared: $(CJSON_SHARED) $(UTILS_SHARED)

static: $(CJSON_STATIC) $(UTILS_STATIC)

tests: $(CJSON_TEST)

test: tests
	./$(CJSON_TEST)

.c.o:
	$(CC) -c $(R_CFLAGS) $<

#tests
#cJSON
$(CJSON_TEST): $(CJSON_TEST_SRC) cJSON.h
	$(CC) $(R_CFLAGS) $(CJSON_TEST_SRC)  -o $@ $(LDLIBS) -I.

#static libraries
#cJSON
$(CJSON_STATIC): $(CJSON_OBJ)
	$(AR) rcs $@ $<
#cJSON_Utils
$(UTILS_STATIC): $(UTILS_OBJ)
	$(AR) rcs $@ $<

#shared libraries .so.1.0.0
#cJSON
$(CJSON_SHARED_VERSION): $(CJSON_OBJ)
	$(CC) -shared -o $@ $< $(CJSON_SO_LDFLAG) $(LDFLAGS)
#cJSON_Utils
$(UTILS_SHARED_VERSION): $(UTILS_OBJ)
	$(CC) -shared -o $@ $< $(CJSON_OBJ) $(UTILS_SO_LDFLAG) $(LDFLAGS)

#objects
#cJSON
$(CJSON_OBJ): cJSON.c cJSON.h
#cJSON_Utils
$(UTILS_OBJ): cJSON_Utils.c cJSON_Utils.h cJSON.h


#links .so -> .so.1 -> .so.1.0.0
#cJSON
$(CJSON_SHARED_SO): $(CJSON_SHARED_VERSION)
	ln -s $(CJSON_SHARED_VERSION) $(CJSON_SHARED_SO)
$(CJSON_SHARED): $(CJSON_SHARED_SO)
	ln -s $(CJSON_SHARED_SO) $(CJSON_SHARED)
#cJSON_Utils
$(UTILS_SHARED_SO): $(UTILS_SHARED_VERSION)
	ln -s $(UTILS_SHARED_VERSION) $(UTILS_SHARED_SO)
$(UTILS_SHARED): $(UTILS_SHARED_SO)
	ln -s $(UTILS_SHARED_SO) $(UTILS_SHARED)

#install
#cJSON
install-cjson:
	mkdir -p $(INSTALL_LIBRARY_PATH) $(INSTALL_INCLUDE_PATH)
	$(INSTALL) cJSON.h $(INSTALL_INCLUDE_PATH)
	$(INSTALL) $(CJSON_SHARED) $(CJSON_SHARED_SO) $(CJSON_SHARED_VERSION) $(INSTALL_LIBRARY_PATH)
#cJSON_Utils
install-utils: install-cjson
	$(INSTALL) cJSON_Utils.h $(INSTALL_INCLUDE_PATH)
	$(INSTALL) $(UTILS_SHARED) $(UTILS_SHARED_SO) $(UTILS_SHARED_VERSION) $(INSTALL_LIBRARY_PATH)

install: install-cjson install-utils

#uninstall
#cJSON
uninstall-cjson: uninstall-utils
	$(RM) $(INSTALL_LIBRARY_PATH)/$(CJSON_SHARED)
	$(RM) $(INSTALL_LIBRARY_PATH)/$(CJSON_SHARED_VERSION)
	$(RM) $(INSTALL_LIBRARY_PATH)/$(CJSON_SHARED_SO)
	$(RM) $(INSTALL_INCLUDE_PATH)/cJSON.h
	
#cJSON_Utils
uninstall-utils:
	$(RM) $(INSTALL_LIBRARY_PATH)/$(UTILS_SHARED)
	$(RM) $(INSTALL_LIBRARY_PATH)/$(UTILS_SHARED_VERSION)
	$(RM) $(INSTALL_LIBRARY_PATH)/$(UTILS_SHARED_SO)
	$(RM) $(INSTALL_INCLUDE_PATH)/cJSON_Utils.h

remove-dir:
	$(if $(wildcard $(INSTALL_LIBRARY_PATH)/*.*),,rmdir $(INSTALL_LIBRARY_PATH))
	$(if $(wildcard $(INSTALL_INCLUDE_PATH)/*.*),,rmdir $(INSTALL_INCLUDE_PATH))

uninstall: uninstall-utils uninstall-cjson remove-dir

clean:
	$(RM) $(CJSON_OBJ) $(UTILS_OBJ) #delete object files
	$(RM) $(CJSON_SHARED) $(CJSON_SHARED_VERSION) $(CJSON_SHARED_SO) $(CJSON_STATIC) #delete cJSON
	$(RM) $(UTILS_SHARED) $(UTILS_SHARED_VERSION) $(UTILS_SHARED_SO) $(UTILS_STATIC) #delete cJSON_Utils
	$(RM) $(CJSON_TEST)  #delete test

# --- C-to-Rust port helpers -------------------------------------------------

.PHONY: oracle rust-driver parity hook-trace c-unity ffi-unity export-check

oracle: build/oracle

build/oracle: tools/cjson-oracle.c cJSON.c cJSON.h
	mkdir -p build
	$(CC) -std=c89 -O2 -Wall -Werror -I. -o build/oracle.tmp tools/cjson-oracle.c cJSON.c $(LDLIBS)
	mv -f build/oracle.tmp build/oracle

rust-driver:
	cargo build --release --target-dir target -p cjson-driver

parity: oracle rust-driver
	printf '%s' '{"status":"completed","loop_count":0,"workspace_roots":["'"$(CURDIR)"'"]}' \
		| python3 tools/parity_gate.py --force

hook-trace: build/hook-trace-c build/hook-trace-ffi
	./build/hook-trace-c > build/hook-trace-c.out
	./build/hook-trace-ffi > build/hook-trace-ffi.out
	@diff -u build/hook-trace-c.out build/hook-trace-ffi.out || \
		(echo "hook-trace diverged; see MIGRATION.md proposed CH-NNN (awaiting approval)"; exit 1)

.PHONY: c-unity
c-unity:
	mkdir -p build/c-unity && cd build/c-unity && \
		cmake ../.. -DENABLE_CJSON_UTILS=OFF -DENABLE_CJSON_TEST=ON && \
		cmake --build . -j$$(nproc) && ctest --output-on-failure

build/hook-trace-c: tools/hook-trace.c cJSON.c cJSON.h
	mkdir -p build
	# C99: snprintf (Sonar S5281); library sources remain C89 via $(CC).
	gcc -std=c99 -O2 -Wall -Werror -I. -o $@ tools/hook-trace.c cJSON.c $(LDLIBS)

build/hook-trace-ffi: tools/hook-trace.c target/release/libcjson_ffi.a cJSON.h
	mkdir -p build
	gcc -std=c99 -O2 -Wall -Werror -I. -o $@ tools/hook-trace.c \
		target/release/libcjson_ffi.a $(LDLIBS) -lpthread -ldl

export-check: rust-ffi
	mkdir -p build
	# Declarations only (skip #define CJSON_PUBLIC(...) macro lines).
	grep -E '^CJSON_PUBLIC\(' cJSON.h \
		| sed -E 's/^CJSON_PUBLIC\([^)]*\)[[:space:]]*([A-Za-z_][A-Za-z0-9_]*).*/\1/' \
		| sort -u > build/header-fns.txt
	nm -g target/release/libcjson_ffi.a 2>/dev/null \
		| awk 'NF>=3 && ($$2=="T" || $$2=="t"){print $$3}' \
		| sed 's/^_//' | sort -u > build/ffi-exports.txt
	@echo "header functions: $$(wc -l < build/header-fns.txt)"
	@test "$$(wc -l < build/header-fns.txt)" -eq 78
	@comm -23 build/header-fns.txt build/ffi-exports.txt > build/missing-exports.txt
	@if [ -s build/missing-exports.txt ]; then \
		echo "Missing FFI exports:"; cat build/missing-exports.txt; exit 1; \
	fi
	@echo "export-check: ok"

rust-ffi:
	cargo build --release --target-dir target -p cjson-ffi
