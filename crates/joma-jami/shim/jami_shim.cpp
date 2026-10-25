// C ABI over libjami's C++ API. Rust calls `joma_jami_call` with a JSON request
// `{"m": "<method>", "a": [args...]}` and gets `{"r": <result>}` or `{"e": "<error>"}`.
// Daemon signals arrive on one callback as `{"s": "<signal>", "a": [args...]}`.
// Keeping the surface to strings keeps the FFI small and the C++ side dumb.

#include <jami.h>
#include <callmanager_interface.h>
#include <configurationmanager_interface.h>
#include <conversation_interface.h>
#include <datatransfer_interface.h>
#include <networkservice_interface.h>
#include <presencemanager_interface.h>
#include <videomanager_interface.h>

#include <json/json.h>

extern "C" {
#include <libavutil/frame.h>
#include <libavutil/pixfmt.h>
}

#include <cstring>
#include <deque>
#include <functional>
#include <map>
#include <memory>
#include <mutex>
#include <string>
#include <vector>

using StrMap = std::map<std::string, std::string>;
using VecMap = std::vector<StrMap>;

typedef void (*joma_event_cb)(const char* json, void* user);

static joma_event_cb g_cb = nullptr;
static void* g_user = nullptr;

// ---- JSON helpers ------------------------------------------------------------

static Json::Value
toJson(const std::string& s)
{
    return Json::Value(s);
}
static Json::Value
toJson(int v)
{
    return Json::Value(v);
}
static Json::Value
toJson(uint32_t v)
{
    return Json::Value(v);
}
static Json::Value
toJson(uint16_t v)
{
    return Json::Value((uint32_t) v);
}
static Json::Value
toJson(uint64_t v)
{
    return Json::Value((Json::UInt64) v);
}
static Json::Value
toJson(int64_t v)
{
    return Json::Value((Json::Int64) v);
}
static Json::Value
toJson(bool v)
{
    return Json::Value(v);
}
static Json::Value
toJson(const StrMap& m)
{
    Json::Value o(Json::objectValue);
    for (const auto& [k, v] : m)
        o[k] = v;
    return o;
}
static Json::Value
toJson(const std::map<std::string, int32_t>& m)
{
    Json::Value o(Json::objectValue);
    for (const auto& [k, v] : m)
        o[k] = v;
    return o;
}
static Json::Value
toJson(const std::vector<std::string>& v)
{
    Json::Value a(Json::arrayValue);
    for (const auto& s : v)
        a.append(s);
    return a;
}
static Json::Value
toJson(const VecMap& v)
{
    Json::Value a(Json::arrayValue);
    for (const auto& m : v)
        a.append(toJson(m));
    return a;
}
static Json::Value
toJson(const std::vector<uint8_t>& v)
{
    // Payloads are small (trust requests carry a vCard); send them as text.
    return Json::Value(std::string(v.begin(), v.end()));
}
static Json::Value
toJson(const libjami::SwarmMessage& m)
{
    Json::Value o(Json::objectValue);
    o["id"] = m.id;
    o["type"] = m.type;
    o["linearizedParent"] = m.linearizedParent;
    o["body"] = toJson(m.body);
    o["reactions"] = toJson(m.reactions);
    o["editions"] = toJson(m.editions);
    o["status"] = toJson(m.status);
    return o;
}
static Json::Value
toJson(const std::vector<libjami::SwarmMessage>& v)
{
    Json::Value a(Json::arrayValue);
    for (const auto& m : v)
        a.append(toJson(m));
    return a;
}

static std::string
write(const Json::Value& v)
{
    Json::StreamWriterBuilder b;
    b["indentation"] = "";
    b["emitUTF8"] = true;
    return Json::writeString(b, v);
}

static std::string
str(const Json::Value& a, int i)
{
    return a.isValidIndex(i) && a[i].isString() ? a[i].asString() : std::string();
}
static int64_t
num(const Json::Value& a, int i)
{
    return a.isValidIndex(i) && a[i].isNumeric() ? a[i].asInt64() : 0;
}
static bool
flag(const Json::Value& a, int i)
{
    return a.isValidIndex(i) && a[i].isBool() && a[i].asBool();
}
static StrMap
smap(const Json::Value& a, int i)
{
    StrMap m;
    if (a.isValidIndex(i) && a[i].isObject())
        for (const auto& k : a[i].getMemberNames())
            m[k] = a[i][k].isString() ? a[i][k].asString() : write(a[i][k]);
    return m;
}
static VecMap
vmap(const Json::Value& a, int i)
{
    VecMap v;
    if (a.isValidIndex(i) && a[i].isArray())
        for (Json::ArrayIndex j = 0; j < a[i].size(); ++j) {
            Json::Value one(Json::arrayValue);
            one.append(a[i][j]);
            v.push_back(smap(one, 0));
        }
    return v;
}

// ---- Signals -----------------------------------------------------------------

template<typename... Args>
static void
emit(const char* name, const Args&... args)
{
    if (!g_cb)
        return;
    Json::Value o(Json::objectValue);
    o["s"] = name;
    Json::Value a(Json::arrayValue);
    (a.append(toJson(args)), ...);
    o["a"] = a;
    auto s = write(o);
    g_cb(s.c_str(), g_user);
}

// One handler per signal, each forwarding its arguments as JSON.
#define FORWARD(Struct)                                                                             \
    handlers.insert(libjami::exportable_callback<Struct>([](auto&&... args) { emit(Struct::name, args...); }))

static void
registerHandlers()
{
    using namespace libjami;
    std::map<std::string, std::shared_ptr<CallbackWrapperBase>> handlers;
    // Accounts and contacts
    handlers.insert(exportable_callback<ConfigurationSignal::AccountsChanged>([]() { emit("AccountsChanged"); }));
    FORWARD(ConfigurationSignal::RegistrationStateChanged);
    FORWARD(ConfigurationSignal::AccountDetailsChanged);
    FORWARD(ConfigurationSignal::VolatileDetailsChanged);
    FORWARD(ConfigurationSignal::IncomingTrustRequest);
    FORWARD(ConfigurationSignal::ContactAdded);
    FORWARD(ConfigurationSignal::ContactRemoved);
    FORWARD(ConfigurationSignal::RegisteredNameFound);
    FORWARD(ConfigurationSignal::NameRegistrationEnded);
    FORWARD(ConfigurationSignal::ProfileReceived);
    FORWARD(ConfigurationSignal::ComposingStatusChanged);
    FORWARD(ConfigurationSignal::AccountMessageStatusChanged);
    FORWARD(ConfigurationSignal::IncomingAccountMessage);
    // Conversations
    FORWARD(ConversationSignal::SwarmLoaded);
    FORWARD(ConversationSignal::SwarmMessageReceived);
    FORWARD(ConversationSignal::SwarmMessageUpdated);
    FORWARD(ConversationSignal::ConversationReady);
    FORWARD(ConversationSignal::ConversationRemoved);
    FORWARD(ConversationSignal::ConversationRequestReceived);
    FORWARD(ConversationSignal::ConversationMemberEvent);
    FORWARD(ConversationSignal::ConversationProfileUpdated);
    // Calls
    FORWARD(ConversationSignal::OnConversationError);
    // Presence and files
    FORWARD(PresenceSignal::NewBuddyNotification);
    FORWARD(DataTransferSignal::DataTransferEvent);
    FORWARD(CallSignal::StateChange);
    FORWARD(CallSignal::IncomingCall);
    FORWARD(CallSignal::IncomingMessage);
    FORWARD(CallSignal::MediaChangeRequested);
    FORWARD(CallSignal::MediaNegotiationStatus);
    // Video
    FORWARD(VideoSignal::DecodingStarted);
    FORWARD(VideoSignal::DecodingStopped);
    // Exposed services and tunnels
    FORWARD(ServiceSignal::PeerServicesReceived);
    FORWARD(ServiceSignal::TunnelOpened);
    FORWARD(ServiceSignal::TunnelClosed);
    registerSignalHandlers(handlers);
}

// ---- Video sinks -------------------------------------------------------------
// The daemon pulls an AVFrame to scale into, then pushes it back when filled. We
// keep the latest filled BGRA frame per sink; Rust copies it out when it paints.

struct Sink
{
    std::mutex lock;
    AVFrame* latest {nullptr};
    uint64_t serial {0};
};
static std::mutex g_sinks_lock;
static std::map<std::string, std::shared_ptr<Sink>> g_sinks;

static std::shared_ptr<Sink>
findSink(const std::string& id)
{
    std::lock_guard<std::mutex> l(g_sinks_lock);
    auto it = g_sinks.find(id);
    return it == g_sinks.end() ? nullptr : it->second;
}

// `width` x `height` is the decoder's size (DecodingStarted); the daemon scales each
// frame into the one `pull` returns, so that frame must have its format, size and
// buffers set: an empty one makes swscale abort (docs/crashes/2026-10-25-...).
static bool
attachSink(const std::string& id, int width, int height)
{
    if (width <= 0 || height <= 0)
        return false;
    auto sink = std::make_shared<Sink>();
    {
        std::lock_guard<std::mutex> l(g_sinks_lock);
        g_sinks[id] = sink;
    }
    libjami::SinkTarget target;
    target.preferredFormat = AV_PIX_FMT_BGRA;
    target.pull = [width, height]() {
        libjami::FrameBuffer frame(av_frame_alloc());
        if (!frame)
            return libjami::FrameBuffer();
        frame->format = AV_PIX_FMT_BGRA;
        frame->width = width;
        frame->height = height;
        if (av_frame_get_buffer(frame.get(), 0) < 0)
            return libjami::FrameBuffer();
        return frame;
    };
    std::weak_ptr<Sink> weak = sink;
    target.push = [weak](libjami::FrameBuffer frame) {
        auto s = weak.lock();
        if (!s || !frame)
            return;
        std::lock_guard<std::mutex> l(s->lock);
        if (s->latest)
            av_frame_free(&s->latest);
        s->latest = frame.release();
        s->serial++;
    };
    return libjami::registerSinkTarget(id, std::move(target));
}

static void
detachSink(const std::string& id)
{
    std::shared_ptr<Sink> sink;
    {
        std::lock_guard<std::mutex> l(g_sinks_lock);
        auto it = g_sinks.find(id);
        if (it == g_sinks.end())
            return;
        sink = it->second;
        g_sinks.erase(it);
    }
    libjami::registerSinkTarget(id, {});
    std::lock_guard<std::mutex> l(sink->lock);
    if (sink->latest)
        av_frame_free(&sink->latest);
}

// ---- Calls -------------------------------------------------------------------

static Json::Value
dispatch(const std::string& m, const Json::Value& a)
{
    using namespace libjami;
    // Daemon
    if (m == "version")
        return version();
    // Accounts
    if (m == "getAccountList")
        return toJson(getAccountList());
    if (m == "getAccountDetails")
        return toJson(getAccountDetails(str(a, 0)));
    if (m == "getVolatileAccountDetails")
        return toJson(getVolatileAccountDetails(str(a, 0)));
    if (m == "getAccountTemplate")
        return toJson(getAccountTemplate(str(a, 0)));
    if (m == "addAccount")
        return addAccount(smap(a, 0));
    if (m == "setAccountDetails")
        return setAccountDetails(str(a, 0), smap(a, 1)), Json::Value();
    if (m == "setAccountActive")
        return setAccountActive(str(a, 0), flag(a, 1)), Json::Value();
    if (m == "updateProfile")
        return updateProfile(str(a, 0), str(a, 1), str(a, 2), str(a, 3), str(a, 4), (int32_t) num(a, 5)), Json::Value();
    if (m == "registerName")
        return registerName(str(a, 0), str(a, 1));
    if (m == "lookupName")
        return lookupName(str(a, 0), str(a, 1), str(a, 2));
    if (m == "lookupAddress")
        return lookupAddress(str(a, 0), str(a, 1), str(a, 2));
    // Contacts
    if (m == "getContacts")
        return toJson(getContacts(str(a, 0)));
    if (m == "getContactDetails")
        return toJson(getContactDetails(str(a, 0), str(a, 1)));
    if (m == "addContact")
        return addContact(str(a, 0), str(a, 1)), Json::Value();
    if (m == "removeContact")
        return removeContact(str(a, 0), str(a, 1), flag(a, 2)), Json::Value();
    if (m == "sendTrustRequest") {
        auto p = str(a, 2);
        return sendTrustRequest(str(a, 0), str(a, 1), std::vector<uint8_t>(p.begin(), p.end())), Json::Value();
    }
    // Conversations
    if (m == "getConversations")
        return toJson(getConversations(str(a, 0)));
    if (m == "getConversationRequests")
        return toJson(getConversationRequests(str(a, 0)));
    if (m == "conversationInfos")
        return toJson(conversationInfos(str(a, 0), str(a, 1)));
    if (m == "getConversationMembers")
        return toJson(getConversationMembers(str(a, 0), str(a, 1)));
    if (m == "startConversation")
        return startConversation(str(a, 0));
    if (m == "addConversationMember")
        return addConversationMember(str(a, 0), str(a, 1), str(a, 2)), Json::Value();
    if (m == "acceptConversationRequest")
        return acceptConversationRequest(str(a, 0), str(a, 1)), Json::Value();
    if (m == "declineConversationRequest")
        return declineConversationRequest(str(a, 0), str(a, 1)), Json::Value();
    if (m == "removeConversation")
        return removeConversation(str(a, 0), str(a, 1));
    if (m == "loadConversation")
        return loadConversation(str(a, 0), str(a, 1), str(a, 2), (size_t) num(a, 3));
    if (m == "sendMessage")
        return sendMessage(str(a, 0), str(a, 1), str(a, 2), str(a, 3), (int32_t) num(a, 4)), Json::Value();
    if (m == "setMessageDisplayed")
        return setMessageDisplayed(str(a, 0), str(a, 1), str(a, 2), (int) num(a, 3));
    if (m == "setIsComposing")
        return setIsComposing(str(a, 0), str(a, 1), flag(a, 2)), Json::Value();
    // Presence
    if (m == "subscribeBuddy")
        return subscribeBuddy(str(a, 0), str(a, 1), flag(a, 2)), Json::Value();
    if (m == "getSubscriptions")
        return toJson(getSubscriptions(str(a, 0)));
    // Files
    if (m == "sendFile")
        return sendFile(str(a, 0), str(a, 1), str(a, 2), str(a, 3), str(a, 4)), Json::Value();
    if (m == "downloadFile")
        return downloadFile(str(a, 0), str(a, 1), str(a, 2), str(a, 3), str(a, 4));
    // Calls
    if (m == "placeCallWithMedia")
        return placeCallWithMedia(str(a, 0), str(a, 1), vmap(a, 2));
    if (m == "acceptWithMedia")
        return acceptWithMedia(str(a, 0), str(a, 1), vmap(a, 2));
    if (m == "refuse")
        return refuse(str(a, 0), str(a, 1));
    if (m == "hangUp")
        return hangUp(str(a, 0), str(a, 1));
    if (m == "muteLocalMedia")
        return muteLocalMedia(str(a, 0), str(a, 1), str(a, 2), flag(a, 3));
    if (m == "requestMediaChange")
        return requestMediaChange(str(a, 0), str(a, 1), vmap(a, 2));
    if (m == "answerMediaChangeRequest")
        return answerMediaChangeRequest(str(a, 0), str(a, 1), vmap(a, 2));
    if (m == "currentMediaList")
        return toJson(currentMediaList(str(a, 0), str(a, 1)));
    if (m == "getCallDetails")
        return toJson(getCallDetails(str(a, 0), str(a, 1)));
    if (m == "getCallList")
        return toJson(getCallList(str(a, 0)));
    if (m == "sendAccountTextMessage")
        return toJson(sendAccountTextMessage(str(a, 0), str(a, 1), smap(a, 2), flag(a, 3) ? 1 : 0));
    if (m == "sendTextMessage")
        return sendTextMessage(str(a, 0), str(a, 1), smap(a, 2), str(a, 3), flag(a, 4)), Json::Value();
    // Video
    if (m == "getDeviceList")
        return toJson(getDeviceList());
    if (m == "getDefaultDevice")
        return getDefaultDevice();
    if (m == "openVideoInput")
        return openVideoInput(str(a, 0));
    if (m == "closeVideoInput")
        return closeVideoInput(str(a, 0));
    if (m == "attachSink")
        return attachSink(str(a, 0), (int) num(a, 1), (int) num(a, 2));
    if (m == "detachSink")
        return detachSink(str(a, 0)), Json::Value();
    // Exposed services and tunnels
    if (m == "addExposedService")
        return addExposedService(str(a, 0), smap(a, 1));
    if (m == "removeExposedService")
        return removeExposedService(str(a, 0), str(a, 1));
    if (m == "getExposedServices")
        return toJson(getExposedServices(str(a, 0)));
    if (m == "queryPeerServices")
        return queryPeerServices(str(a, 0), str(a, 1));
    if (m == "openServiceTunnel")
        return openServiceTunnel(str(a, 0), str(a, 1), str(a, 2), str(a, 3), str(a, 4), (uint16_t) num(a, 5));
    if (m == "closeServiceTunnel")
        return closeServiceTunnel(str(a, 0), str(a, 1));
    throw std::runtime_error("unknown method " + m);
}

static char*
dup(const std::string& s)
{
    char* out = (char*) std::malloc(s.size() + 1);
    std::memcpy(out, s.c_str(), s.size() + 1);
    return out;
}

extern "C" {

bool
joma_jami_init(uint16_t flags, joma_event_cb cb, void* user)
{
    g_cb = cb;
    g_user = user;
    if (!libjami::init(static_cast<libjami::InitFlag>(flags)))
        return false;
    registerHandlers();
    return libjami::start();
}

void
joma_jami_fini()
{
    libjami::fini();
    g_cb = nullptr;
}

char*
joma_jami_call(const char* request)
{
    Json::Value req;
    Json::CharReaderBuilder b;
    std::string errs;
    std::unique_ptr<Json::CharReader> reader(b.newCharReader());
    Json::Value out(Json::objectValue);
    if (!reader->parse(request, request + std::strlen(request), &req, &errs)) {
        out["e"] = "bad request: " + errs;
        return dup(write(out));
    }
    try {
        out["r"] = dispatch(req["m"].asString(), req["a"]);
    } catch (const std::exception& e) {
        out["e"] = e.what();
    }
    return dup(write(out));
}

void
joma_jami_free(char* s)
{
    std::free(s);
}

// Copy the latest BGRA frame of a sink into `dst` when it is newer than `*serial`.
// Returns 1 when copied, 0 when there is nothing new, -1 when `dst` is too small (then
// `*width`/`*height` hold the size needed).
int
joma_jami_sink_frame(const char* id, uint64_t* serial, uint8_t* dst, size_t cap, int* width, int* height)
{
    auto sink = findSink(id);
    if (!sink)
        return 0;
    std::lock_guard<std::mutex> l(sink->lock);
    auto* f = sink->latest;
    if (!f || sink->serial == *serial || f->format != AV_PIX_FMT_BGRA)
        return 0;
    *width = f->width;
    *height = f->height;
    size_t row = (size_t) f->width * 4;
    if (cap < row * f->height)
        return -1;
    for (int y = 0; y < f->height; ++y)
        std::memcpy(dst + row * y, f->data[0] + (size_t) f->linesize[0] * y, row);
    *serial = sink->serial;
    return 1;
}
}
