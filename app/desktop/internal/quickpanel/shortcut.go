package quickpanel

// Shortcut owns only its registration; replacement never drops a working key
// before the OS has accepted the requested one.
type Shortcut struct {
	register   func(string, func()) error
	unregister func(string)
	open       func()
	Effective  string
}

func NewShortcut(register func(string, func()) error, unregister func(string), open func()) *Shortcut {
	return &Shortcut{register: register, unregister: unregister, open: open}
}
func (s *Shortcut) Set(key string) error {
	if key == s.Effective {
		return nil
	}
	if key != "" {
		if err := s.register(key, s.open); err != nil {
			return ErrShortcut
		}
	}
	previous := s.Effective
	s.Effective = key
	if previous != "" {
		s.unregister(previous)
	}
	return nil
}
func (s *Shortcut) Close() {
	if s.Effective != "" {
		s.unregister(s.Effective)
		s.Effective = ""
	}
}
