# Wenshushu Action plugin

The independent repository is [UniClipboard/unic-plugin-wenshushu](https://github.com/UniClipboard/unic-plugin-wenshushu).
Its initial design revision is
[`d79ce8aa9f4ecac07e7802c144ebf70216fd50e0`](https://github.com/UniClipboard/unic-plugin-wenshushu/tree/d79ce8aa9f4ecac07e7802c144ebf70216fd50e0).
This revision contains preparation documents, not an executable plugin.
This directory is a documentation entry, not an installable plugin or a copy
of its implementation.

The plugin will expose `unic.wenshushu/share` with the title
“通过文叔叔临时分享”. The approved input scope is text, images and files.
It will upload content only when the user invokes the Action and will return
a public download URL as a new text history item.

The [Action contract](../../../docs/contracts/clipboard-action-contract.md)
now defines text, image and file inputs, source readers and bounded Go upload
execution. Use a host revision that includes these extensions. Current [installation rules](../../../docs/contracts/plugin-contract.md#8-从-github-仓库安装)
already support independently distributed Go process plugins.

Continue provider-specific development in the independent repository.
When implementation is accepted, replace the preparation revision with an
immutable compatible release revision and add a verified installation command here.
Do not add a placeholder install command or vendor another implementation.
