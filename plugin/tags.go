package plugin

// Built-in tag IDs are language-independent. Hosts translate only their labels.
const (
	TagText    = "builtin:text"
	TagImage   = "builtin:image"
	TagFiles   = "builtin:files"
	TagFolder  = "builtin:folder"
	TagLink    = "builtin:link"
	TagEmail   = "builtin:email"
	TagCode    = "builtin:code"
	TagColor   = "builtin:color"
	TagVersion = "builtin:version"
	TagUUID    = "builtin:uuid"
	TagIPv4    = "builtin:ipv4"
)

// TagFavorite marks a locally saved favorite and is omitted from tag catalogues.
const TagFavorite = "favorite"
