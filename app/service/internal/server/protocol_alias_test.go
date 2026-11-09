package server

import (
	api "github.com/uniclipboard/unic/internal/localapi"
)

type SubmitInput = api.SubmitInput
type ActionInput = api.ActionInput
type ActionOutput = api.ActionOutput
type HeadlessBackend = api.HeadlessBackend
type AdapterRequest = api.AdapterRequest
type AdapterReply = api.AdapterReply
type DesktopCommand = api.DesktopCommand
type DesktopResult = api.DesktopResult
type PeerStatus = api.PeerStatus
type ChannelUpdate = api.ChannelUpdate
type Candidate = api.Candidate
type PendingPeer = api.PendingPeer
type Approval = api.Approval
type BindingApproval = api.BindingApproval
type Event = api.Event
type Status = api.Status
type PairResult = api.PairResult
type Code = api.Code
type ClipboardView = api.ClipboardView
type SetClipboard = api.SetClipboard
type HistoryPart = api.HistoryPart
type HistoryBackendSync = api.HistoryBackendSync
type HistoryItem = api.HistoryItem
type Action = api.Action
type HistoryQuery = api.HistoryQuery
type HistoryPage = api.HistoryPage
type HistoryUpdate = api.HistoryUpdate
type HistoryContent = api.HistoryContent
type Error = api.Error
type FileCacheState = api.FileCacheState
type Backend = api.Backend
type Client = api.Client
type errBody = api.ErrorBody

var NewClient = api.NewClient
var ErrNoDaemon = api.ErrNoDaemon
