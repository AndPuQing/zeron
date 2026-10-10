#!/bin/sh
# fake dsh acp server for zeron-harness tests.
#
# mirrors the wire shapes of `dsh --profile acp-plus` (dsh-acp-plus 0.2.x):
# initialize advertises the `_meta.steering` extension, and session/new
# carries a provider-GROUPED model select (SessionConfigSelectGroup) whose
# values are the raw provider-scoped JSON tuples dsh persists — plus the flat
# off/low/high/max thought_level ladder. the prompt reply echoes every config
# option zeron set.

emit() { printf '%s\n' "$1"; }
rid() { printf '%s' "$1" | sed 's/.*"id":\([0-9]*\).*/\1/'; }
has() { case "$1" in *"$2"*) return 0 ;; *) return 1 ;; esac; }

PRO='[\"deepseek-official\",\"deepseek-v4-pro\"]'
FLASH='[\"deepseek-official\",\"deepseek-v4-flash\"]'
CHAT='[\"deepseek-platform\",\"deepseek-chat\"]'
OPTIONS='[{"id":"model","name":"Model","category":"model","type":"select","currentValue":"'$FLASH'","options":[{"group":"deepseek-official","name":"DeepSeek","options":[{"value":"'$PRO'","name":"DeepSeek V4 Pro"},{"value":"'$FLASH'","name":"DeepSeek V4 Flash"}]},{"group":"deepseek-platform","name":"DeepSeek Platform","options":[{"value":"'$CHAT'","name":"DeepSeek Chat"}]}]},{"id":"thought_level","name":"Reasoning","category":"thought_level","type":"select","currentValue":"high","options":[{"value":"off","name":"Off"},{"value":"low","name":"Low"},{"value":"high","name":"High"},{"value":"max","name":"Max"}]},{"id":"sandbox","name":"Sandbox","category":"model_config","type":"select","currentValue":"workspace","options":[{"group":"access","name":"Access","options":[{"value":"read-only","name":"Read only"},{"value":"workspace","name":"Workspace write"}]}]}]'

SETS=""
while read -r line; do
  has "$line" '"id":' || continue
  id=$(rid "$line")
  if has "$line" '"method":"initialize"'; then
    emit "{\"id\":$id,\"result\":{\"protocolVersion\":1,\"agentCapabilities\":{\"loadSession\":true},\"authMethods\":[],\"_meta\":{\"steering\":{\"supported\":true,\"idleBehavior\":\"promptRequired\"}}}}"
  elif has "$line" '"method":"session/new"'; then
    emit "{\"id\":$id,\"result\":{\"sessionId\":\"dsh-1\",\"configOptions\":$OPTIONS}}"
    emit '{"method":"session/update","params":{"sessionId":"dsh-1","update":{"sessionUpdate":"available_commands_update","availableCommands":[{"name":"plan","description":"Plan carefully"}]}}}'
  elif has "$line" '"method":"session/set_config_option"'; then
    # the value can be a raw provider tuple full of escaped quotes; take
    # everything up to the line's final quote instead of the first.
    set=$(printf '%s' "$line" | sed 's/.*"configId":"\([^"]*\)".*"value":"\(.*\)".*/\1=\2/')
    SETS="$SETS$set;"
    emit "{\"id\":$id,\"result\":{\"configOptions\":$OPTIONS}}"
  elif has "$line" '"method":"session/prompt"'; then
    emit "{\"method\":\"session/update\",\"params\":{\"sessionId\":\"dsh-1\",\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"sets:$SETS\"}}}}"
    emit "{\"id\":$id,\"result\":{\"stopReason\":\"end_turn\"}}"
    exit 0
  else
    emit "{\"id\":$id,\"result\":{}}"
  fi
done
