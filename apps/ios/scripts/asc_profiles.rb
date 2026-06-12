#!/usr/bin/env ruby
# Manage the App Store provisioning profiles used by the TestFlight workflow.
#
#   ruby Scripts/asc_profiles.rb create  <key_id> <issuer_id> <p8_path>
#   ruby Scripts/asc_profiles.rb install <key_id> <issuer_id> <p8_path>
#
# `create` (run locally, one-time / after cert rotation): creates one
# IOS_APP_STORE profile per bundle id below, attached to every valid
# Apple Distribution certificate on the team. Existing profiles with the
# same name are deleted first, so re-running refreshes them.
#
# `install` (run in CI before exportArchive): downloads the profiles by
# name and installs them into ~/Library/Developer/Xcode/UserData/Provisioning Profiles
# and ~/Library/MobileDevice/Provisioning Profiles so a manual-signing
# export can find them.
#
# Why manual signing at export: automatic signing makes xcodebuild attempt
# cloud signing, which requires an Admin-role API key ("Cloud signing
# permission error" otherwise). Pre-created profiles work with the
# App Manager key. PROFILES names are referenced in
# .github/workflows/testflight.yml's ExportOptions — keep them in sync.

require 'openssl'
require 'json'
require 'base64'
require 'net/http'
require 'uri'
require 'fileutils'

PROFILES = {
  'app.uniclipboard.UniClipboard'          => 'UniClipboard App Store',
  'app.uniclipboard.UniClipboard.Share'    => 'UniClipboard Share App Store',
  'app.uniclipboard.UniClipboard.Keyboard' => 'UniClipboard Keyboard App Store',
}.freeze

API = 'https://api.appstoreconnect.apple.com'

mode, key_id, issuer_id, p8_path = ARGV
abort "usage: #{$0} create|install <key_id> <issuer_id> <p8_path>" unless %w[create install].include?(mode) && p8_path

def jwt(key_id, issuer_id, p8_path)
  key = OpenSSL::PKey.read(File.read(p8_path))
  b64 = ->(d) { Base64.urlsafe_encode64(d).delete('=') }
  header  = b64.({ alg: 'ES256', kid: key_id, typ: 'JWT' }.to_json)
  now     = Time.now.to_i
  payload = b64.({ iss: issuer_id, iat: now, exp: now + 1200, aud: 'appstoreconnect-v1' }.to_json)
  input   = "#{header}.#{payload}"
  der = key.sign(OpenSSL::Digest::SHA256.new, input)
  r, s = OpenSSL::ASN1.decode(der).value.map { |i| i.value.to_s(2) }
  raw = [r, s].map { |c| c.length > 32 ? c[-32..-1] : c.rjust(32, "\x00") }.join
  "#{input}.#{b64.(raw)}"
end

TOKEN = jwt(key_id, issuer_id, p8_path)

def request(method, path, body = nil)
  uri = URI("#{API}#{path}")
  http = Net::HTTP.new(uri.host, uri.port)
  http.use_ssl = true
  req = { 'GET' => Net::HTTP::Get, 'POST' => Net::HTTP::Post, 'DELETE' => Net::HTTP::Delete }[method].new(uri)
  req['Authorization'] = "Bearer #{TOKEN}"
  req['Content-Type'] = 'application/json'
  req.body = body.to_json if body
  res = http.request(req)
  abort "#{method} #{path} -> HTTP #{res.code}: #{res.body}" unless res.code.to_i.between?(200, 299)
  res.body.to_s.empty? ? nil : JSON.parse(res.body)
end

case mode
when 'create'
  certs = request('GET', '/v1/certificates?filter[certificateType]=DISTRIBUTION&limit=200')['data']
  abort 'no Apple Distribution certificates on the team' if certs.empty?
  cert_refs = certs.map { |c| { type: 'certificates', id: c['id'] } }
  puts "distribution certs: #{certs.map { |c| c['attributes']['serialNumber'] }.join(', ')}"

  PROFILES.each do |bundle_id, profile_name|
    bid = request('GET', "/v1/bundleIds?filter[identifier]=#{bundle_id}&limit=200")['data']
            .find { |b| b['attributes']['identifier'] == bundle_id }
    abort "bundle id #{bundle_id} not found on the team" unless bid

    request('GET', "/v1/profiles?filter[name]=#{URI.encode_www_form_component(profile_name)}")['data'].each do |old|
      request('DELETE', "/v1/profiles/#{old['id']}")
      puts "deleted stale profile #{old['id']} (#{profile_name})"
    end

    created = request('POST', '/v1/profiles', {
      data: {
        type: 'profiles',
        attributes: { name: profile_name, profileType: 'IOS_APP_STORE' },
        relationships: {
          bundleId: { data: { type: 'bundleIds', id: bid['id'] } },
          certificates: { data: cert_refs },
        },
      },
    })
    puts "created '#{profile_name}' for #{bundle_id} (id=#{created['data']['id']}, expires #{created['data']['attributes']['expirationDate']})"
  end
when 'install'
  dirs = [
    File.expand_path('~/Library/Developer/Xcode/UserData/Provisioning Profiles'),
    File.expand_path('~/Library/MobileDevice/Provisioning Profiles'),
  ]
  dirs.each { |d| FileUtils.mkdir_p(d) }
  PROFILES.each do |bundle_id, profile_name|
    data = request('GET', "/v1/profiles?filter[name]=#{URI.encode_www_form_component(profile_name)}&filter[profileType]=IOS_APP_STORE")['data']
             .find { |p| p['attributes']['name'] == profile_name }
    abort "profile '#{profile_name}' not found — run the `create` mode locally first" unless data
    content = Base64.decode64(data['attributes']['profileContent'])
    uuid = content[/<key>UUID<\/key>\s*<string>([^<]+)<\/string>/, 1] || data['id']
    dirs.each { |d| File.binwrite(File.join(d, "#{uuid}.mobileprovision"), content) }
    puts "installed '#{profile_name}' (#{bundle_id}) as #{uuid}.mobileprovision"
  end
end
