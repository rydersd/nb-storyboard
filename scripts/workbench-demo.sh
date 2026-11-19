#!/bin/sh
# Builds a small demo workspace for the workbench using only the public CLI:
# two projects, three scenes, ten shots with text, a second take, an alternate cut and stickies
# written by an agent. Usage: scripts/workbench-demo.sh WORKSPACE [STORYBOARD_BINARY]
set -eu
WORKSPACE=${1:?usage: workbench-demo.sh WORKSPACE [STORYBOARD]}
SB=${2:-storyboard}
export NB_AUTHOR=agent:demo
[ -e "$WORKSPACE/night-station" ] && { echo "$WORKSPACE/night-station already exists" >&2; exit 1; }
mkdir -p "$WORKSPACE"
P="$WORKSPACE/night-station"

version() { "$SB" show "$1" | sed -n 's/.*"version":"\([^"]*\)".*/\1/p'; }
run() { verb=$1; shift; "$SB" "$verb" "$P" --base-version "$(version "$P")" "$@" > /dev/null; }

"$SB" new "$P" --title "Night Station" > /dev/null
run add-scene --title "Platform"
run add-scene --title "The Train"
run add-scene --title "Arrival"

shot() { # scene title action dialogue speaker camera duration
  run add-shot --scene "$1" --title "$2"
  id=$(python3 -c "import json; d=json.load(open('$P/project.json')); print([s['id'] for sc in d['scenes'] for s in sc.get('shots', [])][-1])")
  run set-shot --shot "$id" --action "$3" --dialogue "$4" --speaker "$5" --camera "$6" --duration "$7"
  run add-text --shot "$id" --take "$(selected_take "$id")" --panel "$(first_panel "$id")" --text "$2" --x 80 --y 80 --width 1280 --height 220 --font-size 110
  run add-text --shot "$id" --take "$(selected_take "$id")" --panel "$(first_panel "$id")" --text "$3" --x 80 --y 360 --width 1280 --height 460 --font-size 60
}
selected_take() { python3 -c "import json; d=json.load(open('$P/project.json')); print([s['selected_take'] for sc in d['scenes'] for s in sc.get('shots', []) if s['id']=='$1'][0])"; }
first_panel() { python3 -c "import json; d=json.load(open('$P/project.json')); print([t['panels'][0]['id'] for sc in d['scenes'] for s in sc.get('shots', []) if s['id']=='$1' for t in s['takes'] if t['id']==s['selected_take']][0])"; }

shot scene-1 "Empty platform" "Fluorescent hum. Nobody on the platform." "" "" "Wide, static" 3
shot scene-1 "Mara waits" "Mara checks the board; every train is cancelled." "" "" "Medium, slow push" 2.5
shot scene-1 "The board flickers" "One line changes: 23:59 NORTH." "" "" "Insert" 1.5
shot scene-1 "She looks up" "Headlights in the tunnel." "Finally." "Mara" "Close-up" 2
shot scene-2 "Doors open" "The doors open on an empty carriage." "" "" "Wide from inside" 2
shot scene-2 "The conductor" "A conductor with no face under the cap." "Ticket?" "Conductor" "Over the shoulder" 3
shot scene-2 "No ticket" "Mara shows an empty hand." "I don't have one." "Mara" "Two-shot" 2.5
shot scene-3 "Tunnel ends" "Light floods the windows." "" "" "Wide" 2
shot scene-3 "The other station" "The same platform, but morning." "" "" "Wide, static" 3
shot scene-3 "Mara steps out" "She steps onto the platform, alone." "" "" "Medium" 2

run add-take --shot shot-6 --name "Faceless, lit from below"
run add-cut --name "Cold open" --id cold-open
for s in shot-4 shot-1 shot-2 shot-3 shot-5 shot-6 shot-7 shot-8 shot-9 shot-10; do run add-clip --cut cold-open --shot "$s"; done

run note --text "Is the cold open stronger if we start on the headlights?" --kind question --pin shot:shot-4
run note --text "Hold the board insert longer; the change reads too fast." --kind todo --pin shot:shot-3
run note --text "Pass 1 blocked out. Shots 6 and 7 need a second look." --kind note

Q="$WORKSPACE/lighthouse"
"$SB" new "$Q" --title "The Lighthouse" > /dev/null
"$SB" add-scene "$Q" --base-version "$(version "$Q")" --title "Storm" > /dev/null
"$SB" add-shot "$Q" --base-version "$(version "$Q")" --scene scene-1 --title "Waves" > /dev/null
echo "demo workspace ready: $WORKSPACE (run: storyboard serve $WORKSPACE)"
