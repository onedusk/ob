#!/usr/bin/env zsh

# echo "# uber_scanner" >> README.md
git init;
git add -A;
git commit -m "first commit";
git branch -M main;
git remote add origin git@github.com:thnkr-one/uber_scanner.git;
git push -u origin main;
