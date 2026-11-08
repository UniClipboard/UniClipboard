// Package settingsdemo demonstrates a single configuration definition shared
// by schema generation and plugin initialization.
package settingsdemo

//go:generate go run ./cmd/schema settings.schema.json ../../../app/mac/Sources/SettingsSchema/Resources/settings-demo.schema.json

type Config struct {
	Prefix  string   `json:"prefix" title:"Label prefix" description:"Added to automatically generated labels." required:"true" default:"\"clip\"" minLength:"1" maxLength:"32" order:"1" group:"Labels"`
	Limit   int      `json:"max_length" title:"Maximum length" required:"true" default:"500" minimum:"1" maximum:"10000" order:"2" group:"Labels"`
	Mode    string   `json:"mode" title:"Text mode" required:"true" default:"\"plain\"" enum:"[\"plain\",\"markdown\"]" optionLabels:"[\"Plain text\",\"Markdown\"]" order:"3" group:"Labels"`
	Enabled bool     `json:"enabled" title:"Add labels" required:"true" default:"true" order:"4" group:"Labels"`
	Notes   string   `json:"notes,omitempty" title:"Notes" default:"\"\"" widget:"textarea" maxLength:"2000" order:"5" group:"Advanced"`
	Token   string   `json:"token,omitempty" title:"Access token" description:"Optional. Kept only for this preview session." secret:"true" widget:"password" minLength:"1" order:"6" group:"Advanced"`
	Tags    []string `json:"tags" title:"Extra labels" required:"true" default:"[]" maxItems:"8" order:"7" group:"Advanced"`
	Retry   Retry    `json:"retry" title:"Retry policy" required:"true" default:"{}" order:"8"`
}

type Retry struct {
	Count int     `json:"count" title:"Attempts" required:"true" default:"3" minimum:"0" maximum:"10" order:"1"`
	Delay float64 `json:"delay" title:"Delay (seconds)" required:"true" default:"0.5" minimum:"0" maximum:"60" order:"2"`
}
