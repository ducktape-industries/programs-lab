# modules-lab — the gates of the programs and views here. Every wasm artifact is a build
# output under $(BUILD_TARGET); none is committed.
CARGO ?= cargo
WASM_OPT ?= wasm-opt
PROGRAMS := trade arcade huddle
VIEWS := trade-view arcade-view browser-view huddle-view
# The programs whose view crate (<program>-view) also builds the program's
# notifier (`notifier` feature, `export_notifier!`): `wasm-notifiers` builds
# each as <program>.notify.wasm, and the founding packs it as the program's
# `ducktape.notify` section.
NOTIFIERS := huddle
# The views with no program behind them (the browser): a founding takes each
# as it is, a `[[views]]` entry (live/founding.toml).
BARE_VIEWS := $(filter-out $(addsuffix -view,$(PROGRAMS)),$(VIEWS))

BUILD_TARGET := $(abspath $(or $(CARGO_TARGET_DIR),target))
# The Cargo.toml profile every wasm build below comes out of, as programs
# has it: `release`, the bytes main and duckpond take, or `fast`, the bytes
# dev and garage take (`make wasm-views WASM_PROFILE=fast`), whose views and
# notifiers wasm-opt skips. Each profile has its own output dir. Only the
# command line sets it, never the environment: a shell that exports it must
# not turn the next release into fast bytes.
WASM_PROFILE = release
WASM_OUT = $(BUILD_TARGET)/wasm32-unknown-unknown/$(WASM_PROFILE)

# The view gate (wasm-opt, then the ABI check) is modules' own, run out of
# the SDK checkout cargo fetched at the pinned rev; `--fast` skips wasm-opt
# for any profile but release.
SDK_TOOLS = $(shell $(CARGO) metadata --format-version 1 | python3 -c 'import json, os, sys; print(os.path.join(next(os.path.dirname(p["manifest_path"]) for p in json.load(sys.stdin)["packages"] if p["name"] == "view"), "..", "..", "..", "tools"))')

VIEW_GATE = sh $(SDK_TOOLS)/view-gate.sh $(if $(filter release,$(WASM_PROFILE)),,--fast)

# An app checkout at the rev that pins the same SDK line, and the debug
# build of it that holds `--render-tree`.
APP ?= ../app
APP_TARGET ?= $(APP)/target

# The same bytes from any checkout: panic locations would otherwise carry
# this checkout's, cargo's and the toolchain's absolute paths.
CARGO_HOME_DIR := $(or $(CARGO_HOME),$(HOME)/.cargo)
SYSROOT := $(shell rustc --print sysroot)
# Where the rust-src component is installed (rust-analyzer asks for it), rustc
# names std's sources by their place under the sysroot, not by /rustc/<commit>
# as it does without it. The last matching remap wins: both read /rustc/<commit>.
RUSTC_COMMIT := $(shell rustc -vV | sed -n 's/^commit-hash: //p')
WASM_RUSTFLAGS := --remap-path-prefix=$(CURDIR)=/build --remap-path-prefix=$(CARGO_HOME_DIR)=/cargo --remap-path-prefix=$(SYSROOT)=/rustc --remap-path-prefix=$(SYSROOT)/lib/rustlib/src/rust=/rustc/$(RUSTC_COMMIT)

WASM_BUILD = RUSTFLAGS="$(WASM_RUSTFLAGS)" $(CARGO) build --profile $(WASM_PROFILE) --target wasm32-unknown-unknown

# A program is a cdylib its view also links, with `program` off; in one
# target dir each build would rebuild the crate for the other. So a program
# builds in a target dir of its own, and its describe module in another.
program_wasm = $(BUILD_TARGET)/$1/$2/wasm32-unknown-unknown/$(WASM_PROFILE)/$(subst -,_,$2).wasm

# A live stage is qa's kit's (`make live`): QA is a qa checkout and KIT its
# kit (default: $(QA)/kit, which builds it first). The stage is STAGE, its
# programs the rev this workspace pins (kit founds the founding programs
# published for it), its app APP_REF (it must pin that same rev) and its
# core CORE_REF (none: kit's default, core's main).
QA ?=
KIT ?=
STAGE ?= lab
APP_REF ?= dev
CORE_REF ?=
LIVE = PROGRAMS="$(PROGRAMS)" QA="$(QA)" KIT="$(KIT)" STAGE="$(STAGE)" APP_REF="$(APP_REF)" CORE_REF="$(CORE_REF)" OUT="$(BUILD_TARGET)/live" WASM_OUT="$(WASM_OUT)" MAKE="$(MAKE)" sh live/live.sh

.PHONY: test wasm-programs wasm-describes wasm-views wasm-notifiers wasm-packed dl-stage dl-publish-check shots live live-down bot

## fmt, clippy and the native tests: each view on a fake host.
test:
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --workspace --all-targets -- -D warnings
	$(CARGO) test --workspace

## builds every program for wasm32 into $(WASM_OUT)/<name>.wasm, unpacked:
## packing its view and describe module into it is the founding's job
## (`make wasm-packed`, by `crates/tools/pack`).
wasm-programs:
	@mkdir -p $(WASM_OUT)
	@$(foreach p,$(PROGRAMS),$(WASM_BUILD) --target-dir $(BUILD_TARGET)/programs/$(p) -p $(p) --features program && cp $(call program_wasm,programs,$(p)) $(WASM_OUT)/ || exit 1;)

## builds every program's describe module (the `alloc` and `describe`
## exports, no imports) into $(WASM_OUT)/<name>.describe.wasm, ABI-checked.
wasm-describes:
	@mkdir -p $(WASM_OUT)
	@$(foreach p,$(PROGRAMS),$(WASM_BUILD) --target-dir $(BUILD_TARGET)/describe/$(p) -p $(p) --features describe && cp $(call program_wasm,describe,$(p)) $(WASM_OUT)/$(subst -,_,$(p)).describe.wasm && python3 $(SDK_TOOLS)/check-describe-abi.py $(WASM_OUT)/$(subst -,_,$(p)).describe.wasm || exit 1;)

## builds every view for wasm32 under $(WASM_OUT)/, optimized (a fast build
## is not) and ABI-checked as modules gates its own, and prints `name  bytes`.
wasm-views:
	@$(foreach v,$(VIEWS),$(WASM_BUILD) -p $(v) && WASM_OPT=$(WASM_OPT) $(VIEW_GATE) $(v) $(WASM_OUT)/$(subst -,_,$(v)).wasm || exit 1;)

## builds every notifier into $(WASM_OUT)/<program>.notify.wasm, optimized as
## a view is, ABI-checked (alloc/init/tick) as modules gates its own, in a target
## dir of its own (the view crate with `notifier` on is another unit), and
## prints `name  bytes`.
wasm-notifiers:
	@mkdir -p $(WASM_OUT)
	@$(foreach p,$(NOTIFIERS),$(WASM_BUILD) --target-dir $(BUILD_TARGET)/notify/$(p) -p $(p)-view --features notifier && cp $(BUILD_TARGET)/notify/$(p)/wasm32-unknown-unknown/$(WASM_PROFILE)/$(p)_view.wasm $(WASM_OUT)/$(p).notify.wasm && WASM_OPT=$(WASM_OPT) $(VIEW_GATE) $(p).notify $(WASM_OUT)/$(p).notify.wasm notifier || exit 1;)

# Where wasm-packed leaves the bodies a network installs: each program packed
# (<program>.wasm) and each bare view (<view>.wasm).
PACKED = $(WASM_OUT)/packed
BODIES = $(foreach a,$(PROGRAMS) $(BARE_VIEWS),$(PACKED)/$(subst -,_,$(a)).wasm)
PACK = $(CARGO) run -q -p pack --

## packs each program as a founding takes it into $(PACKED)/<name>.wasm: its
## view (<program>-view), then its describe module, then its notifier where
## it has one, each a custom section, by programs' own packer
## (`crates/tools/pack`); and copies each bare view beside them as it is.
## These are the bodies a network votes in: `make live` founds a stage with
## them and `make dl-stage` stages them for dl.ducktape.industries.
wasm-packed: wasm-programs wasm-describes wasm-views wasm-notifiers
	@rm -rf $(PACKED) && mkdir -p $(PACKED)
	@$(foreach p,$(subst -,_,$(PROGRAMS)),$(PACK) ducktape.view $(WASM_OUT)/$(p).wasm $(WASM_OUT)/$(p)_view.wasm $(PACKED)/$(p).wasm && $(PACK) ducktape.describe $(PACKED)/$(p).wasm $(WASM_OUT)/$(p).describe.wasm $(PACKED)/$(p).wasm || exit 1;)
	@$(foreach p,$(subst -,_,$(NOTIFIERS)),$(PACK) ducktape.notify $(PACKED)/$(p).wasm $(WASM_OUT)/$(p).notify.wasm $(PACKED)/$(p).wasm || exit 1;)
	@cp $(foreach v,$(BARE_VIEWS),$(WASM_OUT)/$(subst -,_,$(v)).wasm) $(PACKED)/
	@cd $(PACKED) && ls -l $(notdir $(BODIES))

# `make dl-stage` without a channel stops here, before its build
ifneq ($(filter dl-stage,$(MAKECMDGOALS)),)
ifeq ($(filter dev main,$(CHANNEL)),)
$(error usage: make dl-stage CHANNEL=dev|main [WASM_PROFILE=fast] [DL_OUT=dir])
endif
endif

# Where dl-stage writes the stage.
DL_OUT = $(BUILD_TARGET)/dl/$(CHANNEL)

## builds this commit's bodies and stages them for
## https://dl.ducktape.industries/programs-lab/$(CHANNEL)/ (tools/publish-dl.py
## stage), as programs stages its own: `make dl-stage CHANNEL=main` for the
## release profile, `CHANNEL=dev WASM_PROFILE=fast` for the fast one.
## $(DL_OUT) then holds each body and the manifest, and the upload order is
## printed; this needs no credentials. The release workflow's upload job
## uploads the stage (`tools/publish-dl.py upload`).
dl-stage: wasm-packed
	@python3 tools/publish-dl.py stage $(CHANNEL) $(WASM_PROFILE) $(DL_OUT) $(BODIES)

## runs tools/publish-dl.py against a scratch repository and a stand-in for
## `aws` (tools/publish-dl-check.py): the version, the manifest, the upload
## order, and the refusal of a stage or a version's file with other bytes.
dl-publish-check:
	@python3 tools/publish-dl-check.py

## a view with no node: writes the frames trade-view draws and has the
## app's real renderer paint them into shots/*.png (Linux: Xvfb, xdotool,
## ImageMagick; see $(APP)/dev/screens/README.md).
shots:
	$(CARGO) run -q -p trade-view --example frames -- shots/fixtures
	CARGO_TARGET_DIR=$(abspath $(APP_TARGET)) $(APP)/dev/screens/chat-screens.sh shots/fixtures shots

## every program here in the real app: builds and packs each, founds stage
## $(STAGE) of qa's kit with them (trade with a week of history), seeds two
## accounts, a book and two orders that trade, and prints the command that
## opens the app on it. The stage's node runs until `make live-down`.
live:
	@$(LIVE) up

## stops that node, and the bot when it runs.
live-down:
	@$(LIVE) down

## the market maker on the stage `make live` made, until Ctrl-C.
bot:
	@$(LIVE) bot
