file(GLOB_RECURSE MY_SRCS
    "./hal/*.c"
    "./hal/*.cc"
    "./hal/*.cpp"
    "./apps/*.c"
    "./apps/*.cc"
    "./apps/*.cpp"
)

set(MY_INCLUDE_DIRS 
    "."
    "./assets"
    "./hal"
    "./apps"
)

set(PIKA_PYTHON_INCS
    ./apps/app_repl/PikaScript/pikascript-api/
    ./apps/app_repl/PikaScript/pikascript-core/
)

file(GLOB EXTERN_FILES assets/*)

idf_component_register(SRCS "main.cpp" ${MY_SRCS}
                    INCLUDE_DIRS ${MY_INCLUDE_DIRS} ${PIKA_PYTHON_INCS}
                    EMBED_FILES ${EXTERN_FILES}
)
