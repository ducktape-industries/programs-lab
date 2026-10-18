#!/bin/sh
# The workspace's programs in the real app, from a clone. `make live` and `make live-down`
# run this, and the Makefile passes every variable it reads.
#
#   live.sh up     builds each program, its describe module and its view, packs them into
#                  one <program>.wasm, founds stage $STAGE of qa's kit with them (trade
#                  with a week of every market's minutes), and seeds trade: two accounts
#                  with their funds, 36 orders of one resting around BTC/USD's last price,
#                  two market orders of the other's that trade. Prints the command that
#                  opens the app on it. Run again, it founds a fresh network on the stage.
#   live.sh bot    runs the market maker (crates/bots/trade-bot) as wallet bo on the stage `up`
#                  seeded, until Ctrl-C
#   live.sh down   stops the bot when it runs, and the stage's node, which runs until then
#
# qa is not edited. kit founds a stage from the founding programs published for the
# stage's programs rev: `kit build` copies its files and its founding.toml into the stage's
# programs/, and `kit up` fills that founding.toml. So the lab's packed programs go into
# programs/ too, and live/founding.toml's entries after the published ones.
set -eu

die() {
  echo "live: $*" >&2
  exit 1
}

# The workspace's root: Cargo.toml and the make targets are here, and a relative QA is
# from here.
cd "$(dirname "$0")/.."

[ -n "$QA" ] || die "QA=<a qa checkout> is needed: the stage is one of its kit's"
[ -x "$QA/kit" ] || die "$QA is not a qa checkout: it has no kit"
QA=$(cd "$QA" && pwd -P)
KIT=${KIT:-$QA/kit}
dir=$QA/.local/stages/$STAGE

case "${1:-}" in
up) ;;
bot)
  node=$(ps -ww -p "$(cat "$dir/node.pid")" -o args= | sed 's| run --listen .*||')
  [ -x "$node" ] || die "stage $STAGE's node is not running: make live first"
  DUCKTAPE_PASSWORD=$(cat "$dir/password") exec cargo run -q -p trade-bot -- "$node" "$dir/home" bo
  ;;
down)
  pkill -f "trade-bot .* $dir/home bo" || :
  "$KIT" down "$STAGE" --root "$QA"
  exit
  ;;
*)
  sed -n 2,17p live/live.sh
  exit 2
  ;;
esac

for tool in cargo git python3 wasm-opt wasm-tools ps; do
  command -v "$tool" >/dev/null || die "$tool is not on PATH (the README names what a live stage needs)"
done

# The programs rev a Cargo.toml pins.
pin() { sed -n 's|.*ducktape-industries/programs", rev = "\([0-9a-f]*\)".*|\1|p' "$1" | sort -u; }
sdk=$(pin Cargo.toml)
case $sdk in
'' | *[!0-9a-f]*) die "Cargo.toml does not pin one programs rev (its sdk-line)" ;;
esac

mkdir -p "$OUT"

# The stage, at this workspace's programs: kit founds the founding programs published for
# that rev.
set -- new "$STAGE" --root "$QA" --programs "$sdk"
[ -z "$APP_REF" ] || set -- "$@" --app "$APP_REF"
[ -z "$CORE_REF" ] || set -- "$@" --core "$CORE_REF"
"$KIT" "$@"

# The app refuses a view built on another SDK line: its commit must pin the same rev. kit
# pins each repo as a commit of its clone in .local/repos.
app=$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))["app"]["commit"])' "$dir/stage.json")
pinned=$(git -C "$QA/.local/repos/app" show "$app:Cargo.toml" | pin /dev/stdin)
[ "$pinned" = "$sdk" ] || die "the app at ${APP_REF:-main} pins programs ${pinned:-none}
and this workspace pins programs $sdk.
The app refuses a view built on another SDK line: pass APP_REF=<a ref of the app that pins $sdk>."

"$MAKE" wasm-packed
cargo run -q -p trade-stage --bin history -- "$OUT/trade.params"
cargo run -q -p trade-stage --bin seed -- "$OUT/trade.params" "$OUT/seed"

# The node, the app and the founding programs published, with its founding.toml, in the
# stage's programs/ (copied anew each build). `up` copies those only for a stage that has
# none, so a stage made anew at other refs gets them here.
"$KIT" build "$STAGE" --root "$QA"

# Each program as a founding takes it, its view, describe module and notifier packed in,
# and the browser's view as it is (it has no program: a `[[views]]` entry of
# live/founding.toml): what `make wasm-packed` left in $WASM_OUT/packed.
cp "$WASM_OUT"/packed/*.wasm "$dir/programs/"
cp "$OUT/trade.params" "$dir/programs/trade.params"
# The lab's entries after the published ones; `up` fills each file name with its path
# under programs/.
cat live/founding.toml >>"$dir/programs/founding.toml"

"$KIT" up "$STAGE" --root "$QA"

# kit's CLI signs as wallet `dev` alone, and the book's maker is another wallet. So the
# seed goes through the node's own binary, the one kit started: WALLET ARGS...
node=$(ps -ww -p "$(cat "$dir/node.pid")" -o args= | sed 's| run --listen .*||')
[ -x "$node" ] || die "the node kit started is not running: see $dir/node.log"
as() {
  key=$dir/home/keys/$1.key
  shift
  DUCKTAPE_HOME="$dir/home" DUCKTAPE_PASSWORD="$(cat "$dir/password")" DUCKTAPE_USER_KEY="$key" "$node" "$@"
}
# WALLET PROGRAM OP: a refused op is exit 0 too, so the answer is read.
submit() {
  answer=$(as "$1" submit --preconfirm "$2" "$OUT/seed/$3")
  case $answer in
  "preconfirmed applied"*) ;;
  *) die "$1's $3 was not applied: $answer" ;;
  esac
}

echo "seeding stage $STAGE"
as dev wallet new bo >/dev/null
submit dev identity account-ann.bin
submit dev trade fund.bin
submit bo identity account-bo.bin
submit bo trade fund.bin
for op in "$OUT"/seed/rest-*.bin; do submit bo trade "${op##*/}"; done
for op in "$OUT"/seed/take-*.bin; do submit dev trade "${op##*/}"; done

cat <<EOF

$PROGRAMS live on stage $STAGE: wallet dev has an account, its funds and two orders
that traded, and wallet bo's orders rest on both sides of BTC/USD.

  open the app    $KIT app $STAGE --root $QA
                  (the wallet's password is $(cat "$dir/password"); each view is on the rail)
  stop the node   make live-down QA=$QA STAGE=$STAGE
                  (it runs until then)
EOF
