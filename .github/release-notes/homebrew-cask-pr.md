After making any changes to a cask, existing or new, verify:

- [x] The submission is for [a stable version](https://docs.brew.sh/Acceptable-Casks#stable-versions) or [documented exception](https://docs.brew.sh/Acceptable-Casks#but-there-is-no-stable-version).
- [ ] `brew audit --cask --online <cask>` is error-free.
- [ ] `brew style --fix <cask>` reports no offenses.

Additionally, if adding a new cask:

- [ ] Named the cask according to the [token reference](https://docs.brew.sh/Cask-Cookbook#token-reference).
- [ ] Checked the cask was not [already refused](https://github.com/search?q=repo%3AHomebrew%2Fhomebrew-cask+is%3Aclosed+is%3Aunmerged&type=pullrequests) (add the real token to the search field).
- [ ] `brew audit --cask --new <cask>` worked successfully.
- [ ] `HOMEBREW_NO_INSTALL_FROM_API=1 brew install --cask <cask>` worked successfully.
- [ ] `brew uninstall --cask <cask>` worked successfully.

-----

- [x] I did not use AI/LLM to create this PR, or I disclosed the tool/model below and reviewed its output, including [`zap` stanza](https://docs.brew.sh/Cask-Cookbook#stanza-zap) paths; every commit has a human author or co-author and I will answer maintainer questions and review comments myself without AI/LLM.

OpenAI Codex assisted with the package release workflow. CI checks the released DMG hashes and runs Homebrew's Cask style check. The maintainer reviewed the Cask and approved the submission.
