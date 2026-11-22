package plugin

// Built-in tag IDs are language-independent. Hosts translate only their labels.
const (
	TagText   = "builtin:text"
	TagImage  = "builtin:image"
	TagFiles  = "builtin:files"
	TagFolder = "builtin:folder"
	TagLink   = "builtin:link"
	TagCode   = "builtin:code"
	// TagFavorite marks a locally saved favorite and is omitted from tag catalogues.
	TagFavorite = "favorite"
)
