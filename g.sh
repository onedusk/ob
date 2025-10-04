#!/bin/bash

# echo "# ob" >> README.md
git init;
git add -A;
git commit -m "first commit";
git branch -M main;
git remote add origin git@github.com:onedusk/ob.git;
git push -u origin main;
