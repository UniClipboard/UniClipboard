package rpc

// ChannelPair carries a provider pairing code. An empty code explicitly
// resumes the provider's existing pairing.
type ChannelPair struct {
	Code string `json:"code,omitempty"`
}

func (p ChannelPair) Validate() error {
	if p.Code == "" {
		return nil
	}
	if len(p.Code) != 6 {
		return NewError(CodeInvalidArgument)
	}
	for _, digit := range p.Code {
		if digit < '0' || digit > '9' {
			return NewError(CodeInvalidArgument)
		}
	}
	return nil
}

// ChannelContext binds a private provider group to a local external channel.
type ChannelContext struct {
	RemoteContext []byte `json:"remote_context"`
}

func (c ChannelContext) Validate() error {
	if len(c.RemoteContext) == 0 || len(c.RemoteContext) > 256 {
		return NewError(CodeInvalidArgument)
	}
	return nil
}

type ChannelPairResult struct {
	ChannelContext
	ReportedName string `json:"reported_name"`
}

type ChannelResolution struct {
	ID string `json:"id,omitempty"`
}
