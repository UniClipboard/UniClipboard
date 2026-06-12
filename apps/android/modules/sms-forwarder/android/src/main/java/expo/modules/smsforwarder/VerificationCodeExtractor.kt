package expo.modules.smsforwarder

/**
 * 验证码正则匹配与提取工具。
 * 唯一正则来源 — StaticSmsReceiver、SmsForwarderModule、JS 侧均通过此类调用。
 */
object VerificationCodeExtractor {

    /** 验证码正则（与桌面端 SmsCodeService 保持一致） */
    private val REGEX = Regex(
        "(.*)((代|授权|验证|动态|校验)码|[【\\[].*[】\\]]|[Cc][Oo][Dd][Ee]|[Vv]erification\\s?([Cc]ode)?)\\s?(G-|<#>)?([:：\\s是为]|[Ii][Ss]){0,3}[\\(（\\[【{「]?(([0-9\\s]{4,6})|([A-Za-z\\d]{5,6})(?!([Vv]erification)?([Cc][Oo][Dd][Ee])|:))[」}】\\]）\\)]?(?=([^0-9a-zA-Z]|$))(.*)"
    )

    /** 返回提取到的验证码（去除空格），无匹配返回 null */
    fun extract(body: String): String? {
        val match = REGEX.find(body) ?: return null
        // group(7) 对应验证码数字/字母部分
        val code = match.groupValues.getOrNull(7) ?: return null
        if (code.isBlank()) return null
        return code.replace("\\s".toRegex(), "")
    }

    /** 判断短信正文是否包含验证码 */
    fun contains(body: String): Boolean = REGEX.containsMatchIn(body)
}
