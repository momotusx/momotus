package app.marquee.core

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals

class FindAddonsCopyTest {
    @Test
    fun `the words are the shared ones, every key and no more`() {
        val shared =
            Json
                .parseToJsonElement(File("../../shared/find-addons-words.json").readText())
                .jsonObject
                .mapValues { it.value.jsonPrimitive.content }
                // Android shows no eyebrow (design v3); the web and Apple keep theirs until their own screens drop it.
                .filterKeys { it != "eyebrow" }
        val ours =
            with(FindAddonsCopy) {
                mapOf(
                    "title" to TITLE,
                    "lede" to LEDE,
                    "community" to COMMUNITY,
                    "configureHelp" to CONFIGURE_HELP,
                    "configureTv" to CONFIGURE_TV,
                    "pasteLink" to PASTE_LINK,
                    "listLabel" to LIST_LABEL,
                    "official" to OFFICIAL,
                    "communityList" to COMMUNITY_LIST,
                    "showLabel" to SHOW_LABEL,
                    "all" to ALL,
                    "sources" to SOURCES,
                    "subtitles" to SUBTITLES,
                    "catalogs" to CATALOGS,
                    "loading" to LOADING,
                    "failed" to FAILED,
                    "tryAgain" to TRY_AGAIN,
                    "refresh" to REFRESH,
                    "empty" to EMPTY,
                    "filterEmpty" to FILTER_EMPTY,
                    "kindSources" to KIND_SOURCES,
                    "kindSubtitles" to KIND_SUBTITLES,
                    "kindCatalogs" to KIND_CATALOGS,
                    "installing" to INSTALLING,
                    "turningOn" to TURNING_ON,
                    "installed" to INSTALLED,
                    "off" to OFF,
                    "setUp" to SET_UP,
                    "setUpOff" to SET_UP_OFF,
                    "install" to INSTALL,
                    "configure" to CONFIGURE,
                    "turnOn" to TURN_ON,
                    "fromHost" to FROM_HOST,
                    "fromList" to FROM_LIST,
                    "loadingFrom" to LOADING_FROM,
                    "emptyFrom" to EMPTY_FROM,
                    "more" to MORE,
                    "searchLabel" to SEARCH_LABEL,
                    "searchEmpty" to SEARCH_EMPTY,
                    "noRoom" to NO_ROOM,
                    "matching" to MATCHING,
                    "fromHint" to FROM_HINT,
                )
            }
        assertEquals(shared.keys, ours.keys)
        for ((key, value) in shared) assertEquals(value, ours[key], key)
    }
}
