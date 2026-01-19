#!/bin/bash
# Sync music to Navidrome on lechuck-cloud
# After running the updated Ansible role, permissions should be correct

MUSIC_SOURCE="$HOME/Music/" # Adjust to your local music folder
REMOTE_HOST="194.164.53.11"
REMOTE_USER="sripwoud"
REMOTE_PATH="/srv/music/"
SSH_PORT="2209"
SSH_KEY="~/.ssh/identities/ionos"

echo "Syncing music to Navidrome..."
echo "Source: $MUSIC_SOURCE"
echo "Destination: $REMOTE_USER@$REMOTE_HOST:$REMOTE_PATH"

# Simple rsync after permissions are fixed via Ansible
rsync -avzP \
  --delete \
  -e "ssh -p $SSH_PORT -i $SSH_KEY" \
  "$MUSIC_SOURCE" \
  "$REMOTE_USER@$REMOTE_HOST:$REMOTE_PATH"

echo "Music sync complete! Navidrome will scan the library automatically."
