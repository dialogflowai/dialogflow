<script setup>
import { ref } from 'vue';
import { useRoute } from 'vue-router';
import { useI18n } from 'vue-i18n'
import MaterialSymbolsHouseOutline from '~icons/material-symbols/house-outline'
import RiRobot2Line from '~icons/ri/robot-2-line'
import BiChatSquareDots from '~icons/bi/chat-square-dots'
import MaterialSymbolsBook5Outline from '~icons/material-symbols/book-5-outline'
import RiBardLine from '~icons/ri/bard-line'
import SolarDownloadOutline from '~icons/solar/download-outline'
import SolarRouting2Linear from '~icons/solar/routing-2-linear'
import EpSetting from '~icons/ep/setting'
const route = useRoute()
const { t, locale } = useI18n();
const robotId = route.params.robotId
const isCollapse = ref(false)
</script>
<style scoped>
.toggle-button {
    background-color: #4A5064;
    font-size: 10px;
    line-height: 24px;
    color: #fff;
    text-align: center;
    letter-spacing: 0.2em;
    cursor: pointer;
}
</style>
<template>
    <el-container style="min-height: 100vh">
        <el-aside :width="isCollapse ? '65px' : '200px'" style="background-color: #545c64">
            <div class="toggle-button" @click="isCollapse = !isCollapse">
                {{ isCollapse ? '&gt;&gt;&gt;' :
                    '&lt;&lt;&lt;' }}
            </div>
            <el-menu active-text-color="#409Eff" background-color="#545c64" text-color="#fff" :collapse="isCollapse"
                :collapse-transition="false" router :default-active="route.path">
                <el-menu-item index="/">
                    <el-icon>
                        <MaterialSymbolsHouseOutline />
                    </el-icon>
                    <template #title>{{ t('menu.home') }}</template>
                </el-menu-item>
                <el-menu-item :index="'/robot/' + robotId">
                    <el-icon>
                        <RiRobot2Line />
                    </el-icon>
                    <template #title>{{ t('menu.thisRobot') }}</template>
                </el-menu-item>
                <el-menu-item :index="'/robot/' + robotId + '/mainflows'">
                    <el-icon>
                        <BiChatSquareDots />
                    </el-icon>
                    <template #title>{{ t('menu.dialogFlows') }}</template>
                </el-menu-item>
                <el-sub-menu index="kbMenu">
                    <template #title>
                        <el-icon>
                            <MaterialSymbolsBook5Outline />
                        </el-icon>
                        <span>{{ t('menu.kb') }}</span>
                    </template>
                    <el-menu-item :index="'/robot/' + robotId + '/kb/qa'">{{ t('menu.qa') }}</el-menu-item>
                    <el-menu-item :index="'/robot/' + robotId + '/kb/doc'">{{ t('menu.doc') }}</el-menu-item>
                </el-sub-menu>
                <el-menu-item :index="'/robot/' + robotId + '/intents'">
                    <el-icon>
                        <RiBardLine />
                    </el-icon>
                    <template #title>{{ t('menu.intents') }}</template>
                </el-menu-item>
                <el-menu-item :index="'/robot/' + robotId + '/variables'">
                    <el-icon>
                        <SolarDownloadOutline />
                    </el-icon>
                    <template #title>{{ t('menu.vars') }}</template>
                </el-menu-item>
                <el-menu-item :index="'/robot/' + robotId + '/external/httpApis'">
                    <el-icon>
                        <SolarRouting2Linear />
                    </el-icon>
                    <template #title>{{ t('menu.eApi') }}</template>
                </el-menu-item>
                <el-menu-item :index="'/robot/' + robotId + '/settings'">
                    <el-icon>
                        <EpSetting />
                    </el-icon>
                    <template #title>{{ t('menu.rs') }}</template>
                </el-menu-item>
            </el-menu>
        </el-aside>
        <el-main><router-view></router-view></el-main>
    </el-container>
</template>