#!/bin/bash/env zsh

./target/release/oober replace \
  --dir /Users/macadelic/sacredshit/dusk/apps/website \
  --config replace_config.yaml \
  --dry-run;

sleep 2;

./target/release/oober replace \
  --dir /Users/macadelic/sacredshit/dusk/apps/website \
  --config replace_config.yaml;

./target/release/oober clean-backups --dir /Users/macadelic/sacredshit/dusk/apps/website;
